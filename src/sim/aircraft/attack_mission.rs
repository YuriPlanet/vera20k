//! 11-state attack mission state machine for aircraft.
//!
//! Implements the core attack cycle: approach target → check range →
//! fire weapon → return to base. Native branches and outstanding differences
//! are documented below; this legacy dispatcher is not complete Mission_Attack parity.
//!
//! ## State overview
//! - 0: Init — clear flags, validate target
//! - 3: InRangeCheck — check weapon range, close in if needed
//! - 4: FireWeapon — fire, set HasFired, handle result
//! - 10: ReturnToBase — decrement ammo if HasFired, find helipad
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
    has_fired: bool,
    _is_strafe: bool,
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

    match sub_state {
        // ---------------------------------------------------------------
        // State 0: INIT
        // Clear HasFired, IsStrafe. Validate target exists.
        // → State 3 (has target) or State 10 (no target, RTB)
        // ---------------------------------------------------------------
        0 => {
            if target_status.map_or(true, |s| !s.alive) {
                return AttackTickResult::transition(AircraftMission::Attack {
                    sub_state: 10,
                    has_fired: false,
                    is_strafe: false,
                });
            }
            AttackTickResult::stay(AircraftMission::Attack {
                sub_state: 3,
                has_fired: false,
                is_strafe: false,
            })
        }

        // ---------------------------------------------------------------
        // State 3: IN_RANGE_CHECK
        // Check if target is in weapon range. If not, continue approach.
        // If in range: → State 4 (fire).
        // ---------------------------------------------------------------
        3 => {
            let Some(status) = target_status else {
                return AttackTickResult::transition(AircraftMission::Attack {
                    sub_state: 10,
                    has_fired,
                    is_strafe: false,
                });
            };
            if !status.alive {
                return AttackTickResult::transition(AircraftMission::Attack {
                    sub_state: 10,
                    has_fired,
                    is_strafe: false,
                });
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
                AttackTickResult::stay(AircraftMission::Attack {
                    sub_state: 4,
                    has_fired,
                    is_strafe: false,
                })
            } else {
                // Out of range — set movement toward target.
                AttackTickResult::approach(
                    AircraftMission::Attack {
                        sub_state: 3,
                        has_fired,
                        is_strafe: false,
                    },
                    (status.rx, status.ry),
                )
            }
        }

        // ---------------------------------------------------------------
        // State 4: FIRE_WEAPON
        // Check firing arc (±11.25°). If aligned: fire, set HasFired.
        // → State 10 (RTB) or State 5 (strafe) based on FlyBy.
        // ---------------------------------------------------------------
        4 => {
            let Some(status) = target_status else {
                return AttackTickResult::transition(AircraftMission::Attack {
                    sub_state: 10,
                    has_fired,
                    is_strafe: false,
                });
            };
            if !status.alive {
                return AttackTickResult::transition(AircraftMission::Attack {
                    sub_state: 10,
                    has_fired,
                    is_strafe: false,
                });
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
                    AircraftMission::Attack {
                        sub_state: 4,
                        has_fired,
                        is_strafe: false,
                    },
                    (status.rx, status.ry),
                );
            }

            // Firing arc aligned — signal fire permission.
            AttackTickResult::fire(
                AircraftMission::Attack {
                    // The final-release latches carry the evidenced state-1
                    // -> state-10 tail in `tick_aircraft_missions`; no RA2
                    // strafe cadence is inferred for states 5..9.
                    sub_state: 1,
                    has_fired: true,
                    is_strafe: false,
                },
                status.kind,
            )
        }

        // YR states 5..9 are deliberately residual pending the runtime cadence proof.
        5..=9 => AttackTickResult::transition(AircraftMission::Attack {
            sub_state: 10,
            has_fired,
            is_strafe: false,
        }),

        // ---------------------------------------------------------------
        // State 10: RETURN_TO_BASE
        // Decrement ammo if HasFired. Clear flags.
        // If ammo > 0 and target still valid: re-engage (→ State 0).
        // Else: transition to Guard (which handles RTB to airfield).
        // ---------------------------------------------------------------
        10 => {
            let mut result_ammo_delta: i32 = 0;
            if has_fired {
                result_ammo_delta = -1;
            }

            // Re-engage check: still have ammo and target alive?
            let can_reengage =
                ammo_current + result_ammo_delta > 0 && target_status.is_some_and(|s| s.alive);

            if can_reengage {
                AttackTickResult {
                    new_mission: AircraftMission::Attack {
                        sub_state: 0,
                        has_fired: false,
                        is_strafe: false,
                    },
                    ammo_delta: result_ammo_delta,
                    fire_at: None,
                    move_to: None,
                }
            } else {
                AttackTickResult {
                    new_mission: AircraftMission::Guard,
                    ammo_delta: result_ammo_delta,
                    fire_at: None,
                    move_to: None,
                }
            }
        }

        // ---------------------------------------------------------------
        // State 1, 2: Legacy/spawner states — not yet needed.
        // ---------------------------------------------------------------
        _ => AttackTickResult::transition(AircraftMission::Guard),
    }
}

/// Result of one tick of the attack state machine.
pub struct AttackTickResult {
    /// New mission state to write back.
    pub new_mission: AircraftMission,
    /// Ammo change to apply (-1 for decrement on HasFired, 0 otherwise).
    pub ammo_delta: i32,
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
            ammo_delta: 0,
            fire_at: None,
            move_to: None,
        }
    }

    fn transition(mission: AircraftMission) -> Self {
        Self {
            new_mission: mission,
            ammo_delta: 0,
            fire_at: None,
            move_to: None,
        }
    }

    fn approach(mission: AircraftMission, target_cell: (u16, u16)) -> Self {
        Self {
            new_mission: mission,
            ammo_delta: 0,
            fire_at: None,
            move_to: Some(target_cell),
        }
    }

    fn fire(mission: AircraftMission, target: TargetKind) -> Self {
        Self {
            new_mission: mission,
            ammo_delta: 0,
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

        let result = tick_attack_state(&store, &rules, &interner, 1, 0, false, false);
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

        let result = tick_attack_state(&store, &rules, &interner, 1, 0, false, false);
        match result.new_mission {
            AircraftMission::Attack {
                sub_state: 3,
                has_fired: false,
                ..
            } => {}
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

        let result = tick_attack_state(&store, &rules, &interner, 1, 10, true, false);
        // has_fired=true → ammo_delta=-1, ammo was 0 so 0-1=-1 → no re-engage → Guard.
        assert!(matches!(result.new_mission, AircraftMission::Guard));
        assert_eq!(result.ammo_delta, -1);
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

        let result = tick_attack_state(&store, &rules, &interner, 1, 10, true, false);
        // has_fired=true → ammo_delta=-1. ammo was 2, now 1 > 0 → re-engage.
        match result.new_mission {
            AircraftMission::Attack {
                sub_state: 0,
                has_fired: false,
                ..
            } => {}
            other => panic!("Expected re-engage (state 0), got {:?}", other),
        }
        assert_eq!(result.ammo_delta, -1);
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

        let result = tick_attack_state(&store, &rules, &interner, 1, 3, false, false);
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

        let result = tick_attack_state(&store, &rules, &interner, 1, 3, false, false);
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
            tick_attack_state(
                &store,
                &elite_range_rules(),
                &test_interner(),
                1,
                3,
                false,
                false,
            )
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
