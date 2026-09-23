//! Retained TechnoClass burst position, independent of Target and its orders.

use serde::{Deserialize, Serialize};

/// Techno+3B8: ctor6F2F01 initializes zero; FireAt6FF274 increments before
/// GetROF and6FF2C5 stores the signed remainder afterwards. Target assignment
/// to another non-null target does not reset it. Assign_Target6FCF5B clears
/// it on a changed assignment to null (the same-target early exit does not).
/// This owner replaces the old target-owned remaining-shot count. FLH and
/// the next GetROF branch both read this retained index.
#[derive(Debug, Default, Clone, Copy, Hash, Serialize, Deserialize)]
pub struct WeaponBurst {
    index: i32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::combat::{AttackTarget, TargetKind};
    use crate::sim::game_entity::GameEntity;
    use crate::sim::mission::concrete_effects::represented_assign_target;

    #[test]
    fn burst_index_matches_original_signed_increment_and_remainder() {
        let rows: Vec<serde_json::Value> = serde_json::from_str(include_str!(
            "../../../tools/spatial_oracle/techno_burst_index.json"
        ))
        .unwrap();
        assert_eq!(rows.len(), 54);
        for row in rows {
            let mut burst = WeaponBurst {
                index: row["index"].as_i64().unwrap() as i32,
            };
            assert_eq!(
                burst.next_index(),
                row["get_rof_indices"][0].as_i64().unwrap() as i32
            );
            burst.complete_shot(row["burst"].as_i64().unwrap() as i32);
            assert_eq!(
                burst.index(),
                row["next_index"].as_i64().unwrap() as i32,
                "{row}"
            );
        }
    }

    #[test]
    fn player_orders_cancel_the_previous_burst_through_target_assignment() {
        use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
        use crate::sim::command::Command;
        use crate::sim::pathfinding::PathGrid;
        use crate::sim::world::Simulation;
        use std::collections::BTreeMap;

        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[VehicleTypes]\n0=TANK\n[TANK]\nStrength=200\nSpeed=6\nPrimary=Gun\n\
             [Gun]\nDamage=10\nROF=100\nRange=8\nBurst=2\n",
        ))
        .unwrap();
        let heights = BTreeMap::new();
        let grid = PathGrid::new(32, 32);
        for order in 0..4 {
            let mut sim = Simulation::with_seed(0);
            let id = sim
                .spawn_object("TANK", "Americans", 4, 4, 64, &rules, &heights)
                .unwrap();
            let entity = sim.substrate.entities.get_mut(id).unwrap();
            represented_assign_target(entity, Some(TargetKind::Cell(5, 4)));
            entity.passively_acquired_target = true;
            entity.weapon_burst.complete_shot(2);
            assert_eq!(entity.weapon_burst.index(), 1);
            let command = match order {
                0 => Command::Stop { entity_id: id },
                1 => Command::Move {
                    entity_id: id,
                    target_rx: 8,
                    target_ry: 4,
                    queue: false,
                    group_id: None,
                },
                2 => Command::AttackMove {
                    entity_id: id,
                    target_rx: 8,
                    target_ry: 4,
                    queue: false,
                },
                _ => Command::Guard {
                    entity_id: id,
                    target_id: None,
                },
            };
            assert!(sim.apply_command("Americans", &command, Some(&rules), Some(&grid), &heights));
            let entity = sim.substrate.entities.get_mut(id).unwrap();
            assert!(entity.attack_target.is_none(), "{command:?}");
            assert!(!entity.passively_acquired_target, "{command:?}");
            assert_eq!(entity.weapon_burst.index(), 0, "{command:?}");
            // A later non-null assignment preserves the reset value, so FLH
            // and GetROF start the new burst at its first shot.
            represented_assign_target(entity, Some(TargetKind::Cell(6, 4)));
            assert_eq!(entity.weapon_burst.index(), 0, "{command:?}");
        }
    }

    #[test]
    fn special_orders_use_target_setter_after_admission() {
        use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
        use crate::sim::command::Command;
        use crate::sim::world::Simulation;
        use std::collections::BTreeMap;

        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[VehicleTypes]\n0=TANK\n1=APC\n\
             [InfantryTypes]\n0=TANY\n1=ENGINEER\n\
             [BuildingTypes]\n0=DEPOT\n1=ENEMY\n2=BUNKER\n\
             [TANK]\nStrength=200\nSpeed=6\nPrimary=Gun\nBunkerable=yes\n\
             [APC]\nStrength=200\nPassengers=5\nSizeLimit=2\n\
             [TANY]\nStrength=100\nSpeed=4\nC4=yes\nPrimary=Gun\n\
             [ENGINEER]\nStrength=100\nSpeed=4\nEngineer=yes\n\
             [DEPOT]\nStrength=1000\nUnitRepair=yes\n\
             [ENEMY]\nStrength=1000\nCapturable=yes\nCanC4=yes\n\
             [BUNKER]\nStrength=1000\nBunker=yes\n\
             [Gun]\nDamage=10\nROF=100\nRange=8\nBurst=2\n",
        ))
        .unwrap();
        let heights = BTreeMap::new();
        for order in 0..5 {
            for had_target in [false, true] {
                let mut sim = Simulation::with_seed(0);
                let actor_type = match order {
                    1 | 2 => "TANY",
                    3 => "ENGINEER",
                    _ => "TANK",
                };
                let destination_type = match order {
                    0 => "DEPOT",
                    1 => "APC",
                    2 | 3 => "ENEMY",
                    _ => "BUNKER",
                };
                let id = sim
                    .spawn_object(actor_type, "Americans", 4, 4, 64, &rules, &heights)
                    .unwrap();
                let destination = sim
                    .spawn_object(
                        destination_type,
                        if matches!(order, 2 | 3) {
                            "Russians"
                        } else {
                            "Americans"
                        },
                        8,
                        4,
                        64,
                        &rules,
                        &heights,
                    )
                    .unwrap();
                let actor = sim.substrate.entities.get_mut(id).unwrap();
                actor.health.current /= 2; // Repair admission requires damage.
                if had_target {
                    represented_assign_target(actor, Some(TargetKind::Cell(6, 4)));
                }
                actor.passively_acquired_target = true;
                actor.weapon_burst.complete_shot(2);
                let command = match order {
                    0 => Command::RepairAtDepot {
                        entity_id: id,
                        depot_id: destination,
                    },
                    1 => Command::EnterTransport {
                        passenger_id: id,
                        transport_id: destination,
                    },
                    2 => Command::PlantC4 {
                        attacker_id: id,
                        target_building_id: destination,
                    },
                    3 => Command::CaptureBuilding {
                        engineer_id: id,
                        target_building_id: destination,
                    },
                    _ => Command::EnterBunker {
                        unit_id: id,
                        bunker_id: destination,
                    },
                };
                // Rejected ownership cannot cancel a burst or passive provenance.
                assert!(!sim.apply_command("Russians", &command, Some(&rules), None, &heights));
                let actor = sim.substrate.entities.get(id).unwrap();
                assert_eq!(actor.attack_target.is_some(), had_target, "{command:?}");
                assert!(actor.passively_acquired_target, "{command:?}");
                assert_eq!(actor.weapon_burst.index(), 1, "{command:?}");

                assert!(sim.apply_command("Americans", &command, Some(&rules), None, &heights));
                let actor = sim.substrate.entities.get_mut(id).unwrap();
                assert!(actor.attack_target.is_none(), "{command:?}");
                assert!(!actor.passively_acquired_target, "{command:?}");
                assert_eq!(
                    actor.weapon_burst.index(),
                    i32::from(!had_target),
                    "{command:?}"
                );
                // The next non-null assignment must retain the observed index.
                represented_assign_target(actor, Some(TargetKind::Cell(7, 4)));
                assert_eq!(
                    actor.weapon_burst.index(),
                    i32::from(!had_target),
                    "{command:?}"
                );
            }
        }
    }

    #[test]
    fn target_burst_reset_matches_original_setter_and_callers() {
        let rows: Vec<serde_json::Value> = serde_json::from_str(include_str!(
            "../../../tools/spatial_oracle/techno_target_burst.json"
        ))
        .unwrap();
        assert_eq!(rows.len(), 108);
        let target = |value: &serde_json::Value| match value.as_u64().unwrap() {
            0 => None,
            1 => Some(TargetKind::Cell(5, 4)),
            2 => Some(TargetKind::Cell(6, 4)),
            other => panic!("unexpected native target {other}"),
        };
        for row in rows {
            let mut entity = GameEntity::test_default(1, "MTNK", "Americans", 4, 4);
            represented_assign_target(&mut entity, target(&row["previous"]));
            entity.weapon_burst.index = row["index"].as_i64().unwrap() as i32;
            entity.passively_acquired_target = row["passive"].as_u64().unwrap() != 0;
            represented_assign_target(&mut entity, target(&row["requested"]));
            assert_eq!(
                entity.weapon_burst.index(),
                row["next_index"].as_i64().unwrap() as i32,
                "{row}"
            );
            assert_eq!(
                entity.passively_acquired_target,
                row["next_passive"].as_u64().unwrap() != 0,
                "{row}"
            );
            assert_eq!(
                entity.attack_target.as_ref().map(|target| target.target),
                target(&row["target"]),
                "{row}"
            );
        }
    }

    #[test]
    fn target_assignment_keeps_nonnull_burst_and_clears_changed_null() {
        let mut entity = GameEntity::test_default(1, "ORCA", "Americans", 10, 10);
        entity.attack_target = Some(AttackTarget::new(2));
        entity.weapon_burst.complete_shot(5);
        crate::sim::mission::concrete_effects::represented_assign_target_admitted(
            &mut entity,
            Some(TargetKind::Entity(3)),
            true,
        );
        assert_eq!(entity.weapon_burst.index(), 1);
        represented_assign_target(&mut entity, None);
        assert_eq!(entity.weapon_burst.index(), 0);
        entity.weapon_burst.complete_shot(5);
        represented_assign_target(&mut entity, None);
        assert_eq!(
            entity.weapon_burst.index(),
            1,
            "same-target early exit precedes reset"
        );
    }
}

impl WeaponBurst {
    pub(crate) fn index(self) -> i32 {
        self.index
    }

    pub(crate) fn next_index(self) -> i32 {
        self.index.wrapping_add(1)
    }

    pub(crate) fn clear_target(&mut self) {
        self.index = 0;
    }

    pub(crate) fn complete_shot(&mut self, burst: i32) {
        // Native faults on Burst=0. Aircraft never calls FireAt for that
        // value; the ordinary caller retains its existing max(1) policy.
        self.index = self.next_index().wrapping_rem(burst);
    }
}
