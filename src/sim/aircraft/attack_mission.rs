//! 11-state attack mission state machine for aircraft.
//!
//! Implements the core attack cycle: approach target → check range →
//! fire weapon → return to base. Native branches and outstanding differences
//! are documented below; this legacy dispatcher is not complete Mission_Attack parity.
//!
//! ## State overview
//! - 0: Init — clear the action latch, validate target
//! - 1, 3: Fire-location search and approach — owned by world::aircraft_attack
//! - 4: FireWeapon — request shared admission and synchronous burst emission
//! - 10: ReturnToBase — consume pending ammo before the return decision
//!
//! ## Dependency rules
//! - Part of sim/ — depends on sim/components, sim/combat, rules/.
//! - sim/ NEVER depends on render/, ui/, sidebar/, audio/, net/.

use crate::sim::aircraft::AircraftMission;
use crate::sim::combat::{AttackTarget, TargetKind};
use crate::sim::entity_store::EntityStore;

#[cfg(test)]
#[path = "approach_range_tests.rs"]
mod approach_range_tests;

/// Resolved status of an aircraft's current attack target — abstracts over
/// Entity vs Cell so the state machine doesn't care which kind it is.
#[derive(Debug, Clone, Copy)]
pub(crate) struct AircraftTargetStatus {
    /// True if the target is engageable (entity alive, or always-true for cells).
    pub alive: bool,
    /// Original `TargetKind` — passed through to the fire signal so the
    /// projectile pipeline knows whether the destination is an entity or coords.
    pub kind: TargetKind,
}

/// Look up the aircraft's current target status.
///
/// Returns `None` if `attack_target` is `None`. For Entity targets, returns
/// `None` if the entity has despawned. For Cell targets, always returns `Some`
/// (cells "always exist"; the player explicitly chose this cell).
pub(crate) fn aircraft_target_status(
    at: Option<&AttackTarget>,
    entities: &EntityStore,
) -> Option<AircraftTargetStatus> {
    let at = at?;
    match at.target {
        TargetKind::Entity(id) => {
            let target = entities.get(id)?;
            Some(AircraftTargetStatus {
                alive: !target.dying && target.health.current > 0,
                kind: TargetKind::Entity(id),
            })
        }
        TargetKind::Cell(rx, ry) => Some(AircraftTargetStatus {
            alive: true,
            kind: TargetKind::Cell(rx, ry),
        }),
    }
}

/// Native Mission_Attack entry prefixes418006/418031/4180A1/418BEC.
/// The readiness latch+6D2 already belongs to MissionLeafState; Ammo+2FC and
/// pending+6C8 have one owner independent of the current mission variant.
pub(crate) fn enter_attack_state(entity: &mut crate::sim::game_entity::GameEntity, state: u8) {
    if matches!(state, 0 | 1 | 3 | 10) && entity.mission_leaf.as_aircraft().is_some() {
        entity.mission_leaf.set_aircraft_action_latch(false);
    }
    if matches!(state, 1 | 3 | 10) {
        if let Some(ammo) = entity.aircraft_ammo.as_mut() {
            ammo.consume_release(state == 10);
        }
    }
}

/// Advance the attack mission state machine for one aircraft entity.
///
/// Returns the new mission state (may be the same, or transition to Guard/RTB).
/// The caller is responsible for writing the returned mission back to the entity.
pub fn tick_attack_state(
    entities: &EntityStore,
    entity_id: u64,
    sub_state: u8,
) -> AttackTickResult {
    let Some(entity) = entities.get(entity_id) else {
        return AttackTickResult::transition(AircraftMission::Idle);
    };

    // Resolve target as Entity-position-or-Cell-coord, with alive flag.
    // Cell targets always resolve (cells don't despawn); Entity targets resolve
    // to None if the target has been removed.
    let target_status = aircraft_target_status(entity.attack_target.as_ref(), entities);
    let ammo_current = entity.aircraft_ammo.as_ref().map_or(-1, |a| a.current);
    if sub_state == 4 && ammo_current == 0 {
        return AttackTickResult::transition(AircraftMission::Attack { sub_state: 10 });
    }

    match sub_state {
        // ---------------------------------------------------------------
        // State 4: FIRE_WEAPON
        // The combat handoff owns the native secondary-facing check and
        // successful-release suffix; a request alone changes neither state nor ammo.
        // ---------------------------------------------------------------
        4 => {
            let Some(status) = target_status else {
                return AttackTickResult::transition(AircraftMission::Attack { sub_state: 10 });
            };
            if !status.alive {
                return AttackTickResult::transition(AircraftMission::Attack { sub_state: 10 });
            }

            // GetFireError and the native secondary-facing check run at the
            // shared admission boundary, after the state4 facing writers.
            AttackTickResult::fire(
                AircraftMission::Attack {
                    // The emission caller owns the successful-release suffix.
                    // Requesting fire cannot advance state or charge ammo.
                    sub_state: 4,
                },
                status.kind,
            )
        }

        // YR states 5..9 are deliberately residual pending the runtime cadence proof.
        5..=9 => AttackTickResult::transition(AircraftMission::Attack { sub_state: 10 }),

        // ---------------------------------------------------------------
        // State 10: RETURN_TO_BASE
        // Consume pending ammo (positive counts only) in the entry prefix.
        // If ammo != 0 and target still valid: re-engage (→ State 1).
        // Else: transition to Guard (which handles RTB to airfield).
        // ---------------------------------------------------------------
        10 => {
            // Pending ammo was consumed by the entry prefix. Native418C15
            // tests signed nonzero, not positive, before restarting at state1.
            if ammo_current != 0 && target_status.is_some_and(|s| s.alive) {
                AttackTickResult::transition(AircraftMission::Attack { sub_state: 1 })
            } else {
                // Residual: native zero-ammo target-clear/return-location and
                // EnterIdle/queued-Mission suffix418C29..418D1D.
                AttackTickResult::transition(AircraftMission::Guard)
            }
        }

        // States0/1/3 require live world effects and are dispatched through
        // world::aircraft_attack before this read-only legacy handler.
        0 | 1 | 3 => unreachable!("aircraft navigation states require the world transaction"),

        // ---------------------------------------------------------------
        // Other unported states retain the legacy Guard fallback.
        // ---------------------------------------------------------------
        _ => AttackTickResult::transition(AircraftMission::Guard),
    }
}

/// Result of one tick of the attack state machine.
pub struct AttackTickResult {
    /// New mission state to write back.
    pub new_mission: AircraftMission,
    /// If Some, the combat system should fire at this target this tick.
    /// Carries `TargetKind` so the projectile pipeline knows whether the
    /// destination is an entity or a ground cell (force-fire on terrain).
    pub fire_at: Option<TargetKind>,
}

impl AttackTickResult {
    pub(super) fn transition(mission: AircraftMission) -> Self {
        Self {
            new_mission: mission,
            fire_at: None,
        }
    }

    fn fire(mission: AircraftMission, target: TargetKind) -> Self {
        Self {
            new_mission: mission,
            fire_at: Some(target),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::combat::AttackTarget;
    use crate::sim::docking::aircraft_dock::AircraftAmmo;
    use crate::sim::entity_store::EntityStore;
    use crate::sim::game_entity::GameEntity;

    #[test]
    fn test_state10_no_ammo_goes_to_guard() {
        let mut store = EntityStore::new();
        let mut attacker = GameEntity::test_default(1, "ORCA", "Americans", 10, 10);
        attacker.attack_target = Some(AttackTarget::new(2));
        attacker.aircraft_ammo = Some(AircraftAmmo::new(2));
        store.insert(attacker);
        let target = GameEntity::test_default(2, "RHINO", "Soviet", 15, 15);
        store.insert(target);
        // Deplete ammo.
        store
            .get_mut(1)
            .unwrap()
            .aircraft_ammo
            .as_mut()
            .unwrap()
            .current = 0;

        let result = tick_attack_state(&store, 1, 10);
        // Entry housekeeping never decrements zero ammo in state10.
        assert!(matches!(result.new_mission, AircraftMission::Guard));
    }

    #[test]
    fn test_state10_has_ammo_reengages() {
        let mut store = EntityStore::new();
        let mut attacker = GameEntity::test_default(1, "ORCA", "Americans", 10, 10);
        attacker.attack_target = Some(AttackTarget::new(2));
        attacker.aircraft_ammo = Some(AircraftAmmo::new(2));
        store.insert(attacker);
        let target = GameEntity::test_default(2, "RHINO", "Soviet", 15, 15);
        store.insert(target);

        let result = tick_attack_state(&store, 1, 10);
        // Nonzero ammo re-engages through native state1.
        match result.new_mission {
            AircraftMission::Attack { sub_state: 1, .. } => {}
            other => panic!("Expected re-engage (state 1), got {:?}", other),
        }
    }
}
