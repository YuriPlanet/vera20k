//! 11-state attack mission state machine for aircraft.
//!
//! Implements the core attack cycle: approach target → check range →
//! fire weapon → return to base. Native branches and outstanding differences
//! are documented below; this legacy dispatcher is not complete Mission_Attack parity.
//!
//! ## State overview
//! - 0: Init — clear the action latch, validate target
//! - 3: InRangeCheck — check weapon range, close in if needed
//! - 4: FireWeapon — request emission after the legacy arc check
//! - 10: ReturnToBase — consume pending ammo before the return decision
//!
//! ## Dependency rules
//! - Part of sim/ — depends on sim/components, sim/combat, rules/.
//! - sim/ NEVER depends on render/, ui/, sidebar/, audio/, net/.

use crate::rules::ruleset::RuleSet;
use crate::sim::aircraft::AircraftMission;
use crate::sim::combat::{AttackTarget, TargetKind};
use crate::sim::entity_store::EntityStore;
use crate::sim::intern::StringInterner;

#[cfg(test)]
#[path = "approach_range_tests.rs"]
mod approach_range_tests;

/// ±11.25° firing arc in 16-bit facing units.
/// 0x800 = 2048 out of 65536 = 11.25°.
/// Aircraft can only fire when target bearing is within this arc of their heading.
const FIRING_ARC_TOLERANCE: u16 = 0x800;

/// Resolved status of an aircraft's current attack target — abstracts over
/// Entity vs Cell so the state machine doesn't care which kind it is.
#[derive(Debug, Clone, Copy)]
pub(crate) struct AircraftTargetStatus {
    /// Target cell coord (entity position for Entity, cell coord for Cell).
    pub rx: u16,
    pub ry: u16,
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
                rx: target.position.rx,
                ry: target.position.ry,
                alive: !target.dying && target.health.current > 0,
                kind: TargetKind::Entity(id),
            })
        }
        TargetKind::Cell(rx, ry) => Some(AircraftTargetStatus {
            rx,
            ry,
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
    rules: &RuleSet,
    interner: &StringInterner,
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
    let entity_rx = entity.position.rx;
    let entity_ry = entity.position.ry;
    let entity_facing = entity.facing;
    let entity_veterancy = entity.veterancy;
    let type_ref = entity.type_ref();

    // Look up type info.
    let type_str = interner.resolve(type_ref);
    let obj = rules.object(type_str);
    // Aircraft417FE0 range branch4180F4..418117 uses GetWeapon(0), including
    // elite fallback, then signed Distance_To < raw Range leptons. No generic
    // CanFireAt range bonuses or cell-rounded five-cell fallback belong here.
    // Scope: the legacy state3 handler still needs its native auxiliary+18
    // admission and the other navigation/facing arms418146..4182A2 migrated.
    let weapon_range_leptons = obj
        .and_then(|o| crate::sim::combat::combat_weapon::primary_for_tier(o, entity_veterancy))
        .and_then(|name| rules.weapon(name))
        .map(|weapon| weapon.range_leptons);

    if matches!(sub_state, 1 | 3 | 4) && ammo_current == 0 {
        return AttackTickResult::transition(AircraftMission::Attack { sub_state: 10 });
    }

    match sub_state {
        // ---------------------------------------------------------------
        // State 0: INIT
        // Clear the shared action latch only. Validate target exists.
        // → State 3 (has target) or State 10 (no target, RTB)
        // ---------------------------------------------------------------
        0 => {
            if target_status.map_or(true, |s| !s.alive) {
                return AttackTickResult::transition(AircraftMission::Attack { sub_state: 10 });
            }
            AttackTickResult::stay(AircraftMission::Attack { sub_state: 3 })
        }

        // ---------------------------------------------------------------
        // State 3: IN_RANGE_CHECK
        // Check if target is in weapon range. If not, continue approach.
        // If in range: → State 4 (fire).
        // ---------------------------------------------------------------
        3 => {
            let Some(status) = target_status else {
                return AttackTickResult::transition(AircraftMission::Attack { sub_state: 10 });
            };
            if !status.alive {
                return AttackTickResult::transition(AircraftMission::Attack { sub_state: 10 });
            }

            let distance = crate::sim::combat::object_distance_to(
                entity,
                &status.kind,
                entities,
                rules,
                interner,
            );
            if distance
                .zip(weapon_range_leptons)
                .is_some_and(|(distance, range)| distance < range)
            {
                // In range → fire.
                AttackTickResult::stay(AircraftMission::Attack { sub_state: 4 })
            } else {
                // Out of range — set movement toward target.
                AttackTickResult::approach(
                    AircraftMission::Attack { sub_state: 3 },
                    (status.rx, status.ry),
                )
            }
        }

        // ---------------------------------------------------------------
        // State 4: FIRE_WEAPON
        // Legacy firing arc (±11.25°); emission must decide actual success.
        // Aircraft GetFireError41A9E0 and the release suffix remain to be wired.
        // ---------------------------------------------------------------
        4 => {
            let Some(status) = target_status else {
                return AttackTickResult::transition(AircraftMission::Attack { sub_state: 10 });
            };
            if !status.alive {
                return AttackTickResult::transition(AircraftMission::Attack { sub_state: 10 });
            }

            // Firing arc check: ±11.25° (0x800 in 16-bit facing).
            let target_dx = status.rx as i32 - entity_rx as i32;
            let target_dy = status.ry as i32 - entity_ry as i32;
            let target_facing_u8 = crate::sim::movement::facing_from_delta(target_dx, target_dy);
            // Convert both to 16-bit for arc comparison.
            let entity_facing_16: u16 = (entity_facing as u16) << 8;
            let target_facing_16: u16 = (target_facing_u8 as u16) << 8;
            let facing_diff = (entity_facing_16 as i16)
                .wrapping_sub(target_facing_16 as i16)
                .unsigned_abs();

            if facing_diff > FIRING_ARC_TOLERANCE {
                // Not aligned — continue approach (don't fire).
                return AttackTickResult::approach(
                    AircraftMission::Attack { sub_state: 4 },
                    (status.rx, status.ry),
                );
            }

            // Firing arc aligned — signal fire permission.
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

        // State1's FindFireLocation/AssignDestination suffix4197C0 remains
        // unmigrated. Keep the state instead of fabricating a final-release
        // countdown or clearing the target on a guessed next frame.
        1 => AttackTickResult::stay(AircraftMission::Attack {
            sub_state: if target_status.is_some() { 1 } else { 10 },
        }),

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
    /// If Some, issue an air move command toward this cell.
    pub move_to: Option<(u16, u16)>,
}

impl AttackTickResult {
    fn stay(mission: AircraftMission) -> Self {
        Self {
            new_mission: mission,
            fire_at: None,
            move_to: None,
        }
    }

    fn transition(mission: AircraftMission) -> Self {
        Self {
            new_mission: mission,
            fire_at: None,
            move_to: None,
        }
    }

    fn approach(mission: AircraftMission, target_cell: (u16, u16)) -> Self {
        Self {
            new_mission: mission,
            fire_at: None,
            move_to: Some(target_cell),
        }
    }

    fn fire(mission: AircraftMission, target: TargetKind) -> Self {
        Self {
            new_mission: mission,
            fire_at: Some(target),
            move_to: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;
    use crate::sim::combat::AttackTarget;
    use crate::sim::docking::aircraft_dock::AircraftAmmo;
    use crate::sim::entity_store::EntityStore;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::intern::test_interner;

    fn test_rules() -> RuleSet {
        let ini_str = "\
[AircraftTypes]\n0=ORCA\n\n\
[VehicleTypes]\n0=RHINO\n\n\
[InfantryTypes]\n\n\
[BuildingTypes]\n\n\
[ORCA]\nStrength=150\nArmor=light\nSpeed=14\nPrimary=Hellfire\nAmmo=2\nFlyBy=yes\n\n\
[RHINO]\nStrength=400\nArmor=heavy\nSpeed=6\n\n\
[Hellfire]\nDamage=100\nROF=20\nRange=5\nWarhead=HE\n\n\
[HE]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,0%,0%\n";
        let ini = IniFile::from_str(ini_str);
        RuleSet::from_ini(&ini).expect("test rules")
    }

    #[test]
    fn test_state0_no_target_goes_to_state10() {
        let mut store = EntityStore::new();
        let attacker = GameEntity::test_default(1, "ORCA", "Americans", 10, 10);
        store.insert(attacker);
        let interner = test_interner();
        let rules = test_rules();

        let result = tick_attack_state(&store, &rules, &interner, 1, 0);
        match result.new_mission {
            AircraftMission::Attack { sub_state: 10, .. } => {}
            other => panic!("Expected state 10, got {:?}", other),
        }
    }

    #[test]
    fn test_state0_with_target_goes_to_state3() {
        let mut store = EntityStore::new();
        let mut attacker = GameEntity::test_default(1, "ORCA", "Americans", 10, 10);
        attacker.attack_target = Some(AttackTarget::new(2));
        attacker.aircraft_ammo = Some(AircraftAmmo::new(2));
        store.insert(attacker);
        let target = GameEntity::test_default(2, "RHINO", "Soviet", 15, 15);
        store.insert(target);
        let interner = test_interner();
        let rules = test_rules();

        let result = tick_attack_state(&store, &rules, &interner, 1, 0);
        match result.new_mission {
            AircraftMission::Attack { sub_state: 3, .. } => {}
            other => panic!("Expected state 3, got {:?}", other),
        }
    }

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
        let interner = test_interner();
        let rules = test_rules();

        let result = tick_attack_state(&store, &rules, &interner, 1, 10);
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
        let interner = test_interner();
        let rules = test_rules();

        let result = tick_attack_state(&store, &rules, &interner, 1, 10);
        // Nonzero ammo re-engages through native state1.
        match result.new_mission {
            AircraftMission::Attack { sub_state: 1, .. } => {}
            other => panic!("Expected re-engage (state 1), got {:?}", other),
        }
    }

    #[test]
    fn test_state3_in_range_goes_to_state4() {
        let mut store = EntityStore::new();
        let mut attacker = GameEntity::test_default(1, "ORCA", "Americans", 10, 10);
        attacker.attack_target = Some(AttackTarget::new(2));
        attacker.aircraft_ammo = Some(AircraftAmmo::new(2));
        store.insert(attacker);
        // Target within 5-cell weapon range.
        let target = GameEntity::test_default(2, "RHINO", "Soviet", 13, 10);
        store.insert(target);
        let interner = test_interner();
        let rules = test_rules();

        let result = tick_attack_state(&store, &rules, &interner, 1, 3);
        match result.new_mission {
            AircraftMission::Attack { sub_state: 4, .. } => {}
            other => panic!("Expected state 4, got {:?}", other),
        }
    }

    #[test]
    fn test_state3_out_of_range_approaches() {
        let mut store = EntityStore::new();
        let mut attacker = GameEntity::test_default(1, "ORCA", "Americans", 10, 10);
        attacker.attack_target = Some(AttackTarget::new(2));
        attacker.aircraft_ammo = Some(AircraftAmmo::new(2));
        store.insert(attacker);
        // Target far away (20 cells).
        let target = GameEntity::test_default(2, "RHINO", "Soviet", 30, 10);
        store.insert(target);
        let interner = test_interner();
        let rules = test_rules();

        let result = tick_attack_state(&store, &rules, &interner, 1, 3);
        // Should stay in state 3 and issue a move command.
        match result.new_mission {
            AircraftMission::Attack { sub_state: 3, .. } => {}
            other => panic!("Expected state 3 (approach), got {:?}", other),
        }
        assert!(result.move_to.is_some());
    }

    /// Stock `[ORCA]` weapon data, so the expectation is the retail one:
    /// `Primary=Maverick` (`Range=6`), `ElitePrimary=MaverickE` (`Range=9`).
    fn elite_range_rules() -> RuleSet {
        let ini_str = "\
[AircraftTypes]\n0=ORCA\n\n\
[VehicleTypes]\n0=RHINO\n\n\
[InfantryTypes]\n\n\
[BuildingTypes]\n\n\
[ORCA]\nStrength=150\nArmor=light\nSpeed=14\nPrimary=Maverick\nElitePrimary=MaverickE\nAmmo=2\nFlyBy=yes\n\n\
[RHINO]\nStrength=400\nArmor=heavy\nSpeed=6\n\n\
[Maverick]\nDamage=100\nROF=20\nRange=6\nWarhead=HE\n\n\
[MaverickE]\nDamage=100\nROF=20\nRange=9\nWarhead=HE\n\n\
[HE]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,0%,0%\n";
        let ini = IniFile::from_str(ini_str);
        RuleSet::from_ini(&ini).expect("test rules")
    }

    /// `AircraftClass::Mission_Attack @ 0x00417FE0` case 3 takes its range from
    /// `GetWeapon(0)` (`CALL [vtable+0x3F8]` at `0x004180F9`, argument `EBX = 0`
    /// from `0x004180A7`), and `TechnoClass::GetWeapon @ 0x0070E140` swaps in
    /// `EliteWeapon[0]` for an elite object. So the elite tier decides the
    /// `sub_state` 3 → 4 transition at `0x00418115`/`0x00418117`.
    ///
    /// A target 8 cells east: out of the base weapon's 6, inside `MaverickE`'s 9.
    #[test]
    fn elite_aircraft_fires_at_the_elite_weapon_range() {
        fn run_at(veterancy: u16) -> AttackTickResult {
            let mut store = EntityStore::new();
            let mut attacker = GameEntity::test_default(1, "ORCA", "Americans", 10, 10);
            attacker.veterancy = veterancy;
            attacker.attack_target = Some(AttackTarget::new(2));
            attacker.aircraft_ammo = Some(AircraftAmmo::new(2));
            store.insert(attacker);
            store.insert(GameEntity::test_default(2, "RHINO", "Soviet", 18, 10));
            tick_attack_state(&store, &elite_range_rules(), &test_interner(), 1, 3)
        }

        // Rookie: 8 > Maverick Range 6 → keep approaching.
        match run_at(0).new_mission {
            AircraftMission::Attack { sub_state: 3, .. } => {}
            other => panic!("rookie ORCA at 8 cells should still approach, got {other:?}"),
        }
        // Elite: native lepton distance < MaverickE Range 9 → fire.
        match run_at(200).new_mission {
            AircraftMission::Attack { sub_state: 4, .. } => {}
            other => panic!("elite ORCA at 8 cells should fire, got {other:?}"),
        }
    }
}
