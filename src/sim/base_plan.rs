//! Ordered House `BaseClass` plan state and its bounded lifecycle mutations.
//!
//! The vector is independent of production's ready queues. Ordinary planning,
//! node selection, and placement-result classification remain deliberately
//! outside this module.

use std::hash::Hash;
#[cfg(test)]
use std::hash::Hasher;

/// One native 16-byte BasePlan node represented without pointer aliases.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct BasePlanNode {
    /// BuildingType registry index, or a literal negative planner control.
    pub type_or_control: i32,
    /// Signed-i16 X in the low word and signed-i16 Y in the high word.
    pub packed_cell: u32,
    pub filled: bool,
    pub retry_count: i32,
}

/// The authoritative ordered `HouseClass` BasePlan.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BasePlanState {
    pub percent_built: i32,
    pub nodes: Vec<BasePlanNode>,
}

/// Pack the native `CellStruct` words after signed-16 narrowing.
pub(crate) const fn pack_base_plan_cell(x: i32, y: i32) -> u32 {
    (x as i16 as u16 as u32) | ((y as i16 as u16 as u32) << 16)
}

/// Recover the two signed `CellStruct` words.
#[cfg(test)]
pub(crate) const fn unpack_base_plan_cell(packed: u32) -> (i16, i16) {
    (packed as u16 as i16, (packed >> 16) as u16 as i16)
}

impl BasePlanState {
    /// Fold the exact fields consumed by `FUN_0042F180 @ 0x0042F180`
    /// (`BaseClass::CalculateChecksum`).
    ///
    /// PercentBuilt, filled latches, and retry counters are intentionally not
    /// part of this native compatibility helper. Rust's authoritative world
    /// hash covers them separately.
    #[cfg(test)]
    pub(crate) fn hash_native_checksum_fields(&self, hasher: &mut impl Hasher) {
        (self.nodes.len() as i32).hash(hasher);
        for node in &self.nodes {
            node.type_or_control.hash(hasher);
            let (x, y) = unpack_base_plan_cell(node.packed_cell);
            x.hash(hasher);
            y.hash(hasher);
        }
    }

    /// Apply successful non-human Building Unlimbo satisfaction.
    ///
    /// Native `FUN_0042F260 @ 0x0042F260` scans exact type/cell first, then
    /// the undeploy fallback at `0x0042F2DE..0x0042F31F`; the selected node's
    /// filled/retry writes are `0x0042F321..0x0042F325`. Its active caller is
    /// `BuildingClass::Unlimbo @ 0x00440580`, `0x0044159D..0x004415B3`.
    pub(crate) fn fill_successful_building(
        &mut self,
        building_type_index: i32,
        packed_cell: u32,
        has_undeploy_target: bool,
    ) -> Option<usize> {
        let selected = self.nodes.iter().position(|node| {
            node.type_or_control == building_type_index && node.packed_cell == packed_cell
        });
        let selected = selected.or_else(|| {
            has_undeploy_target.then(|| {
                self.nodes
                    .iter()
                    .position(|node| node.type_or_control == building_type_index && !node.filled)
            })?
        });
        if let Some(index) = selected {
            self.nodes[index].filled = true;
            self.nodes[index].retry_count = 0;
        }
        selected
    }

    /// Apply the BuildingClass Limbo BasePlan invalidation pass.
    ///
    /// Native authority is `FUN_0050A490 @ 0x0050A490`, called by
    /// `BuildingClass__Limbo @ 0x00445880` before `TechnoClass__Limbo`.
    pub(crate) fn invalidate_limbo_building(
        &mut self,
        building_type_index: i32,
        packed_cell: u32,
        is_base_defense: bool,
        game_mode_nonzero: bool,
    ) -> Option<usize> {
        let matched = self.nodes.iter().position(|node| {
            node.type_or_control == building_type_index && node.packed_cell == packed_cell
        })?;

        for (index, node) in self.nodes.iter_mut().enumerate() {
            if index != matched && node.packed_cell == packed_cell {
                node.packed_cell = 0;
            }
        }

        if is_base_defense && game_mode_nonzero {
            self.nodes[matched].type_or_control = -1;
            self.nodes[matched].packed_cell = 0;
        }
        Some(matched)
    }

    /// Clear every cached site equal to one failed ordinary coordinate.
    ///
    /// Native `BuildingClass__ExitObject_Main @ 0x00443C60` performs this
    /// ordinary-node clear at `0x0044552D..0x004455A2`.
    #[cfg(test)]
    pub(crate) fn clear_failed_site(&mut self, packed_cell: u32) -> usize {
        let mut cleared = 0;
        for node in &mut self.nodes {
            if node.packed_cell == packed_cell {
                node.packed_cell = 0;
                cleared += 1;
            }
        }
        cleared
    }

    /// Apply the normalized Building-exit result to one referenced node.
    ///
    /// `FUN_0042F380 @ 0x0042F380` increments the signed retry field first; the
    /// `BuildingClass__ExitObject_Main` result block at
    /// `0x00445237..0x004452C3` then applies the mode/strict-threshold gates
    /// and ordered shift-left removal. `Vec::remove` preserves that tail order.
    #[cfg(test)]
    pub(crate) fn apply_normalized_placement_result(
        &mut self,
        node_index: usize,
        normalized_final_result: i32,
        game_mode_nonzero: bool,
        maximum_failures: i32,
    ) -> bool {
        if normalized_final_result != 1 || node_index >= self.nodes.len() {
            return false;
        }
        let retry_count = self.nodes[node_index].retry_count.wrapping_add(1);
        self.nodes[node_index].retry_count = retry_count;
        if game_mode_nonzero && retry_count > maximum_failures {
            self.nodes.remove(node_index);
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use std::hash::Hasher;

    use super::*;

    fn node(type_or_control: i32, x: i32, y: i32, filled: bool, retry_count: i32) -> BasePlanNode {
        BasePlanNode {
            type_or_control,
            packed_cell: pack_base_plan_cell(x, y),
            filled,
            retry_count,
        }
    }

    fn native_checksum(plan: &BasePlanState) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        plan.hash_native_checksum_fields(&mut hasher);
        hasher.finish()
    }

    #[test]
    fn gsi_04_05_native_base_plan_checksum_excludes_runtime_fields() {
        let mut plan = BasePlanState {
            percent_built: 73,
            nodes: vec![node(8, -2, 32_769, false, -10)],
        };
        let expected = native_checksum(&plan);
        plan.percent_built = -9;
        plan.nodes[0].filled = true;
        plan.nodes[0].retry_count = i32::MAX;
        assert_eq!(native_checksum(&plan), expected);

        let mut changed_order = plan.clone();
        changed_order.nodes.push(node(-3, 7, 9, false, 0));
        let mut reverse = changed_order.clone();
        reverse.nodes.reverse();
        assert_ne!(native_checksum(&changed_order), native_checksum(&reverse));
    }

    #[test]
    fn gsi_04_05_unlimbo_fill_prioritizes_exact_then_undeploy_fallback() {
        let exact_cell = pack_base_plan_cell(20, 30);
        let mut exact = BasePlanState {
            percent_built: 0,
            nodes: vec![node(4, 1, 1, false, 11), node(4, 20, 30, true, -7)],
        };
        assert_eq!(exact.fill_successful_building(4, exact_cell, true), Some(1));
        assert_eq!(exact.nodes[0].retry_count, 11);
        assert!(exact.nodes[1].filled, "filled does not gate the exact scan");
        assert_eq!(exact.nodes[1].retry_count, 0);

        let mut fallback = BasePlanState {
            percent_built: 0,
            nodes: vec![
                node(4, 2, 3, true, 9),
                node(4, 88, 99, false, 6),
                node(4, 21, 31, false, 5),
            ],
        };
        assert_eq!(
            fallback.fill_successful_building(4, exact_cell, true),
            Some(1),
            "fallback skips filled nodes and ignores cell"
        );
        assert!(fallback.nodes[1].filled);
        assert_eq!(fallback.nodes[1].retry_count, 0);

        let unchanged = fallback.clone();
        assert_eq!(
            fallback.fill_successful_building(4, pack_base_plan_cell(7, 7), false),
            None
        );
        assert_eq!(fallback, unchanged);
    }

    #[test]
    fn gsi_04_05_limbo_invalidation_preserves_required_node_fields() {
        let cell = pack_base_plan_cell(5, 6);
        let original = BasePlanState {
            percent_built: 12,
            nodes: vec![
                node(7, 5, 6, true, 4),
                node(9, 5, 6, true, -8),
                node(7, 5, 6, false, 2),
            ],
        };

        for (is_defense, nonzero_mode) in [(false, true), (true, false)] {
            let mut plan = original.clone();
            assert_eq!(
                plan.invalidate_limbo_building(7, cell, is_defense, nonzero_mode),
                Some(0)
            );
            assert_eq!(plan.nodes[0], original.nodes[0]);
            assert_eq!(plan.nodes[1].packed_cell, 0);
            assert_eq!(plan.nodes[1].type_or_control, 9);
            assert!(plan.nodes[1].filled);
            assert_eq!(plan.nodes[1].retry_count, -8);
            assert_eq!(plan.nodes[2].packed_cell, 0);
        }

        let mut skirmish_defense = original;
        skirmish_defense.invalidate_limbo_building(7, cell, true, true);
        assert_eq!(skirmish_defense.nodes[0].type_or_control, -1);
        assert_eq!(skirmish_defense.nodes[0].packed_cell, 0);
        assert!(skirmish_defense.nodes[0].filled);
        assert_eq!(skirmish_defense.nodes[0].retry_count, 4);
    }

    #[test]
    fn gsi_04_05_failure_site_clear_preserves_non_cell_fields() {
        let target = pack_base_plan_cell(-9, 14);
        let mut plan = BasePlanState {
            percent_built: 1,
            nodes: vec![
                node(-3, -9, 14, true, -6),
                node(5, 8, 8, false, 2),
                node(7, -9, 14, false, 11),
            ],
        };
        assert_eq!(plan.clear_failed_site(target), 2);
        assert_eq!(plan.nodes[0], node(-3, 0, 0, true, -6));
        assert_eq!(plan.nodes[1], node(5, 8, 8, false, 2));
        assert_eq!(plan.nodes[2], node(7, 0, 0, false, 11));
    }

    #[test]
    fn gsi_04_05_retry_is_postincrement_signed_strict_and_stable() {
        let source = BasePlanState {
            percent_built: 0,
            nodes: vec![
                node(1, 1, 1, false, 0),
                node(2, 2, 2, true, 2),
                node(3, 3, 3, false, 7),
            ],
        };

        let mut retail = source.clone();
        assert!(!retail.apply_normalized_placement_result(1, 1, true, 3));
        assert_eq!(retail.nodes[1].retry_count, 3);
        assert!(retail.apply_normalized_placement_result(1, 1, true, 3));
        assert_eq!(
            retail
                .nodes
                .iter()
                .map(|n| n.type_or_control)
                .collect::<Vec<_>>(),
            [1, 3]
        );

        let mut equality = source.clone();
        equality.nodes[1].retry_count = 2;
        assert!(!equality.apply_normalized_placement_result(1, 1, true, 3));
        assert_eq!(equality.nodes[1].retry_count, 3);

        let mut campaign = source.clone();
        assert!(!campaign.apply_normalized_placement_result(1, 1, false, -100));
        assert_eq!(campaign.nodes[1].retry_count, 3);

        let mut negative = source.clone();
        negative.nodes[0].retry_count = 0;
        assert!(negative.apply_normalized_placement_result(0, 1, true, -1));
        assert_eq!(
            negative
                .nodes
                .iter()
                .map(|n| n.type_or_control)
                .collect::<Vec<_>>(),
            [2, 3]
        );

        let mut wrapping = source;
        wrapping.nodes[0].retry_count = i32::MAX;
        assert!(!wrapping.apply_normalized_placement_result(0, 1, true, 3));
        assert_eq!(wrapping.nodes[0].retry_count, i32::MIN);
        assert!(!wrapping.apply_normalized_placement_result(99, 1, true, 3));
        assert!(!wrapping.apply_normalized_placement_result(0, 0, true, -1));
    }
}
