//! Ordered House `BaseClass` plan state and its bounded lifecycle mutations.
//!
//! The vector is independent of production's ready queues. Ordinary planning,
//! node selection, and placement-result classification remain deliberately
//! outside this module.

use std::hash::Hash;
#[cfg(any(test, feature = "test-support"))]
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
    percent_built: i32,
    nodes: Vec<BasePlanNode>,
}

/// Pack the native `CellStruct` words after signed-16 narrowing.
pub(crate) const fn pack_base_plan_cell(x: i32, y: i32) -> u32 {
    (x as i16 as u16 as u32) | ((y as i16 as u16 as u32) << 16)
}

/// Recover the two signed `CellStruct` words.
pub(crate) const fn unpack_base_plan_cell(packed: u32) -> (i16, i16) {
    (packed as u16 as i16, (packed >> 16) as u16 as i16)
}

impl BasePlanState {
    /// `BaseClass+0x24`, read from the House section's `PercentBuilt=`.
    pub const fn percent_built(&self) -> i32 {
        self.percent_built
    }

    /// The nodes in plan order.
    pub fn nodes(&self) -> &[BasePlanNode] {
        &self.nodes
    }

    /// `BaseClass::Read_INI` from `HouseClass::Read_Scenario_INI`: the House
    /// section's `PercentBuilt=` and its authored nodes, in order.
    pub(crate) fn read_scenario(
        &mut self,
        percent_built: i32,
        nodes: impl IntoIterator<Item = BasePlanNode>,
    ) {
        self.percent_built = percent_built;
        self.nodes = nodes.into_iter().collect();
    }

    /// `AI_RecalcBuildOptions` empties the vector and appends one unplaced
    /// node per chosen type or control, in order
    /// ([`crate::sim::base_plan_generation::recalc_base_plan`]).
    pub(crate) fn rebuild(&mut self, types_or_controls: impl IntoIterator<Item = i32>) {
        self.nodes.clear();
        self.nodes.extend(
            types_or_controls
                .into_iter()
                .map(|type_or_control| BasePlanNode {
                    type_or_control,
                    packed_cell: 0,
                    filled: false,
                    retry_count: 0,
                }),
        );
    }

    /// Node zero takes the base anchor's cell, before the BasePlan center
    /// (`0x0050C2F5..` after a BuildConst pick, `0x00739926..` after an MCV
    /// deploy). An empty plan has no node to take it.
    pub(crate) fn anchor_first_node(&mut self, packed_cell: u32) {
        if let Some(node) = self.nodes.first_mut() {
            node.packed_cell = packed_cell;
        }
    }

    /// The ordered insert at `index`: the node there and the rest shift up.
    pub(crate) fn insert(&mut self, index: usize, node: BasePlanNode) {
        self.nodes.insert(index, node);
    }

    /// The site search stores its cell in node `index`; a node that may
    /// replan forgets its cell (zero). Nothing past the end.
    pub(crate) fn assign_site(&mut self, index: usize, packed_cell: u32) {
        if let Some(node) = self.nodes.get_mut(index) {
            node.packed_cell = packed_cell;
        }
    }

    /// `HouseClass::AI_ChooseNextProduction @ 0x00506EF0`'s writes
    /// (`0x00507A4C..0x00507B7B`): a placeholder node (negative) becomes the
    /// defense at the cell; a WallTower node takes the cell, and hands the
    /// defense and the cell to the next node when that is `-1`.
    pub(crate) fn place_defense(&mut self, index: usize, defense_type: i32, packed_cell: u32) {
        let Some(node) = self.nodes.get_mut(index) else {
            return;
        };
        if node.type_or_control < 0 {
            node.type_or_control = defense_type;
            node.packed_cell = packed_cell;
            return;
        }
        node.packed_cell = packed_cell;
        if let Some(next) = self.nodes.get_mut(index + 1)
            && next.type_or_control == -1
        {
            next.type_or_control = defense_type;
            next.packed_cell = packed_cell;
        }
    }

    /// Manage_Build_Queue's rebuild node (`0x004FE1D6..0x004FE2BD`): `node`
    /// goes in at `index`. Past the count (1 on a plan the sale emptied)
    /// native writes the slot past the count, which grows to take in slot 0:
    /// `slot_zero`, the node last removed, returns instead. With
    /// `drop_later_same_type`, later nodes of `node`'s type go.
    pub(crate) fn insert_rebuild_node(
        &mut self,
        index: usize,
        node: BasePlanNode,
        slot_zero: Option<BasePlanNode>,
        drop_later_same_type: bool,
    ) {
        if index > self.nodes.len() {
            self.nodes.extend(slot_zero);
            return;
        }
        self.nodes.insert(index, node);
        if drop_later_same_type {
            let later = self.nodes.split_off(index + 1);
            self.nodes.extend(
                later
                    .into_iter()
                    .filter(|later| later.type_or_control != node.type_or_control),
            );
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_percent_built_for_test(&mut self, percent_built: i32) {
        self.percent_built = percent_built;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_nodes_for_test(&mut self, nodes: Vec<BasePlanNode>) {
        self.nodes = nodes;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn nodes_mut_for_test(&mut self) -> &mut Vec<BasePlanNode> {
        &mut self.nodes
    }

    /// Fold the exact fields consumed by `FUN_0042F180 @ 0x0042F180`
    /// (`BaseClass::CalculateChecksum`).
    ///
    /// PercentBuilt, filled latches, and retry counters are intentionally not
    /// part of this native compatibility helper. Rust's authoritative world
    /// hash covers them separately.
    #[cfg(any(test, feature = "test-support"))]
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

    /// Forget a site the Construction Yard's exit could not use: every node
    /// whose cell is the failed one loses it
    /// (`BuildingClass::Exit_Object @ 0x00443C60`, `0x0044552D..0x004455A2`).
    pub(crate) fn clear_failed_site(&mut self, packed_cell: u32) {
        for node in &mut self.nodes {
            if node.packed_cell == packed_cell {
                node.packed_cell = 0;
            }
        }
    }

    /// Count a try-later exit against node `index` (`0x0042F380` returns the
    /// incremented signed count); a skirmish drops the node once the count
    /// exceeds `maximum_failures` (`0x00445249..0x004452C3`, the ordered
    /// shift-left removal). Returns whether the node was dropped.
    pub(crate) fn count_placement_failure(
        &mut self,
        index: usize,
        game_mode_nonzero: bool,
        maximum_failures: i32,
    ) -> bool {
        let Some(node) = self.nodes.get_mut(index) else {
            return false;
        };
        node.retry_count = node.retry_count.wrapping_add(1);
        if game_mode_nonzero && node.retry_count > maximum_failures {
            self.nodes.remove(index);
            return true;
        }
        false
    }

    /// `DynamicVector<BaseNodeClass>::InsertAfter @ 0x0050EB70`: `node` goes
    /// right after node `index`. The vector grows by 10 (`0x0042E709`), so
    /// the insert never fails.
    pub(crate) fn insert_after(&mut self, index: usize, node: BasePlanNode) {
        self.nodes.insert(index + 1, node);
    }

    /// The ordered shift-left removal every caller inlines (`DynamicVector`
    /// `Delete` of one 16-byte node); nothing past the end.
    pub(crate) fn remove(&mut self, index: usize) {
        if index < self.nodes.len() {
            self.nodes.remove(index);
        }
    }

    /// `DynamicVector<BaseNodeClass>::FindIndex @ 0x0042F5E0`, then the
    /// ordered removal: the first node whose type and both cell words equal
    /// `copy`'s goes.
    pub(crate) fn remove_equal(&mut self, copy: BasePlanNode) {
        if let Some(index) = self.nodes.iter().position(|node| {
            node.type_or_control == copy.type_or_control && node.packed_cell == copy.packed_cell
        }) {
            self.nodes.remove(index);
        }
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
        plan.clear_failed_site(target);
        assert_eq!(plan.nodes[0], node(-3, 0, 0, true, -6));
        assert_eq!(plan.nodes[1], node(5, 8, 8, false, 2));
        assert_eq!(plan.nodes[2], node(7, 0, 0, false, 11));
    }

    #[test]
    fn gsi_04_05_retry_counts_before_the_signed_strict_limit() {
        let source = BasePlanState {
            percent_built: 0,
            nodes: vec![
                node(1, 1, 1, false, 0),
                node(2, 2, 2, true, 2),
                node(3, 3, 3, false, 7),
            ],
        };

        let mut retail = source.clone();
        assert!(!retail.count_placement_failure(1, true, 3));
        assert_eq!(retail.nodes[1].retry_count, 3);
        assert!(retail.count_placement_failure(1, true, 3));
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
        assert!(!equality.count_placement_failure(1, true, 3));
        assert_eq!(equality.nodes[1].retry_count, 3);

        let mut campaign = source.clone();
        assert!(!campaign.count_placement_failure(1, false, -100));
        assert_eq!(campaign.nodes[1].retry_count, 3);

        let mut negative = source.clone();
        negative.nodes[0].retry_count = 0;
        assert!(negative.count_placement_failure(0, true, -1));
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
        assert!(!wrapping.count_placement_failure(0, true, 3));
        assert_eq!(wrapping.nodes[0].retry_count, i32::MIN);
        assert!(!wrapping.count_placement_failure(99, true, 3));
    }
}
