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
    fn target_assignment_keeps_nonnull_burst_and_clears_changed_null() {
        let mut entity = GameEntity::test_default(1, "ORCA", "Americans", 10, 10);
        entity.attack_target = Some(AttackTarget::new(2));
        entity.weapon_burst.complete_shot(5);
        represented_assign_target(&mut entity, Some(TargetKind::Entity(3)));
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
