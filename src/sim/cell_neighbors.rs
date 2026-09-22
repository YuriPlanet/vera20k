//! Retained Foot+55C history and Cell+122 mutations. Position/occupation alone
//! cannot reconstruct these counters: airborne Unlimbo and owner changes have
//! deliberately asymmetric native writes. The live byte plane stays on OverlayGrid.

use crate::map::entities::EntityCategory;
use crate::sim::world::Simulation;
use std::collections::VecDeque;

#[derive(
    Debug, Default, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct FootNeighborState {
    cell: (i16, i16),
}

/// Derived bounded journal for movement's cached sum. Readers that fall behind
/// rebuild from the retained plane; no simulation decision reads this journal.
#[derive(Debug, Default, Clone)]
pub(crate) struct NeighborCountChanges {
    revision: u64,
    entries: VecDeque<(usize, u8)>,
}

impl NeighborCountChanges {
    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }

    /// Returns false on revision rollover so the owner can invalidate cache keys.
    pub(crate) fn record(&mut self, index: usize, add: bool) -> bool {
        let continued = self.revision != u64::MAX;
        if !continued {
            self.revision = 0;
            self.entries.clear();
        }
        self.revision += 1;
        // Bound idle-reader memory; missed deltas cause an exact rebuild.
        if self.entries.len() == 4096 {
            self.entries.pop_front();
        }
        self.entries
            .push_back((index, if add { 1 } else { u8::MAX }));
        continued
    }

    pub(crate) fn since(&self, revision: u64) -> Option<impl Iterator<Item = (usize, u8)> + '_> {
        let count = usize::try_from(self.revision.checked_sub(revision)?).ok()?;
        (count <= self.entries.len()).then(|| {
            self.entries
                .iter()
                .skip(self.entries.len() - count)
                .copied()
        })
    }
}

impl Simulation {
    fn foot_neighbor_position(&self, id: u64) -> Option<(i16, i16)> {
        let entity = self.substrate.entities.get(id)?;
        (entity.category != EntityCategory::Structure)
            .then_some((entity.position.rx as i16, entity.position.ry as i16))
    }

    fn adjust_foot_neighbors(&mut self, cell: (i16, i16), add: bool) {
        if let Some(grid) = self.overlay_grid.as_mut() {
            grid.adjust_foot_neighbor_source(self.resolved_terrain.as_ref(), cell, add);
        }
    }

    fn foot_neighbor_high_flight(
        &self,
        id: u64,
        rules: Option<&crate::rules::ruleset::RuleSet>,
    ) -> bool {
        self.substrate.entities.get(id).is_some_and(|entity| {
            // Aircraft41B920 overrides +54 for the two Rules-designated missile
            // types. Rocket661F90 reads phase3..5, independently of Mark/height.
            if entity.category == EntityCategory::Aircraft
                && rules.is_some_and(|rules| {
                    let name = self.interner.resolve(entity.type_ref());
                    name.eq_ignore_ascii_case(&rules.missile_spawn.v3.type_name)
                        || name.eq_ignore_ascii_case(&rules.missile_spawn.dmisl.type_name)
                })
            {
                return entity
                    .rocket_state
                    .as_ref()
                    .is_some_and(|rocket| rocket.phase.is_moving_now());
            }
            // Object virtual+54 checks Mark before GetHeight5F5F40. Use the
            // existing physical-height authority, including ground/OnBridge.
            entity.lifecycle.cell_marked
                && crate::sim::movement::air_movement::current_fly_height(
                    entity,
                    self.resolved_terrain.as_ref(),
                ) >= 208
        })
    }

    /// Foot Unlimbo4D7248..4D72BF increments before the high-flight query;
    /// high flight leaves the prior (constructor-zero) source unchanged. Return
    /// that same native +54 result for the following AirTracker admission.
    pub(crate) fn foot_neighbors_after_unlimbo(
        &mut self,
        id: u64,
        rules: Option<&crate::rules::ruleset::RuleSet>,
    ) -> bool {
        let Some(cell) = self.foot_neighbor_position(id) else {
            return false;
        };
        self.adjust_foot_neighbors(cell, true);
        let high_flight = self.foot_neighbor_high_flight(id, rules);
        if !high_flight {
            self.substrate
                .entities
                .get_mut(id)
                .unwrap()
                .navigation
                .neighbor_state
                .cell = cell;
        }
        high_flight
    }

    /// Foot PerCell(reason2)4D8627..4D8757 skips the WHOLE migration on zero.
    /// Call only at native PerCell boundaries, never on every coordinate change.
    pub(crate) fn foot_neighbors_at_per_cell(&mut self, id: u64) {
        if self.foot_neighbor_position(id).is_none() {
            return;
        }
        let old = self
            .substrate
            .entities
            .get(id)
            .unwrap()
            .navigation
            .neighbor_state
            .cell;
        if old == (0, 0) {
            return;
        }
        self.adjust_foot_neighbors(old, false);
        let cell = self.foot_neighbor_position(id).unwrap();
        self.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .navigation
            .neighbor_state
            .cell = cell;
        self.adjust_foot_neighbors(cell, true);
    }

    /// Foot Limbo4DB298..4DB2E9 decrements even a zero source; the saved cell
    /// survives Limbo. The caller's InLimbo check suppresses repeat removal.
    pub(crate) fn foot_neighbors_before_limbo(&mut self, id: u64) {
        if self.foot_neighbor_position(id).is_none() {
            return;
        }
        let entity = self.substrate.entities.get(id).unwrap();
        if !entity.lifecycle.in_limbo {
            self.adjust_foot_neighbors(entity.navigation.neighbor_state.cell, false);
        }
    }

    /// Foot ChangeOwner4DBF37..4DBF53 replaces the source without counters.
    pub(crate) fn foot_neighbors_after_owner_change(
        &mut self,
        id: u64,
        rules: Option<&crate::rules::ruleset::RuleSet>,
    ) {
        if let Some(cell) = self.foot_neighbor_position(id)
            && !self.foot_neighbor_high_flight(id, rules)
        {
            self.substrate
                .entities
                .get_mut(id)
                .unwrap()
                .navigation
                .neighbor_state
                .cell = cell;
        }
    }

    /// Aircraft DropPayload415E26..415E6E samples the selected Cell's center,
    /// then truncates /256; it does not change Cell+122 in this suffix.
    pub(crate) fn foot_neighbors_after_payload_drop(&mut self, id: u64, cell: (i16, i16)) {
        let cell = self.resolved_terrain.as_ref().map_or(cell, |terrain| {
            terrain.native_cell_coord(terrain.native_cell_identity(cell))
        });
        if let Some(entity) = self.substrate.entities.get_mut(id) {
            let centre_cell = |n: i16| ((i32::from(n) * 256 + 128) / 256) as i16;
            entity.navigation.neighbor_state.cell = (centre_cell(cell.0), centre_cell(cell.1));
        }
    }

    /// Fly landing4CED49..4CEE97 skips only the zero-source decrement, then
    /// always stores/adds the current cell. Landing owns when this is reached.
    pub(crate) fn foot_neighbors_after_fly_landing(&mut self, id: u64) {
        let Some(cell) = self.foot_neighbor_position(id) else {
            return;
        };
        let old = self
            .substrate
            .entities
            .get(id)
            .unwrap()
            .navigation
            .neighbor_state
            .cell;
        if old != (0, 0) {
            self.adjust_foot_neighbors(old, false);
        }
        self.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .navigation
            .neighbor_state
            .cell = cell;
        self.adjust_foot_neighbors(cell, true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::resolved_terrain::{ResolvedTerrainGrid, test_flat_cell};
    use crate::sim::game_entity::GameEntity;
    use crate::sim::overlay_grid::OverlayGrid;
    use crate::sim::snapshot::GameSnapshot;

    fn fixture(width: u16, seed: u8) -> Simulation {
        let mut sim = Simulation::with_seed(31);
        sim.resolved_terrain = Some(ResolvedTerrainGrid::from_cells(
            width,
            width,
            (0..width)
                .flat_map(|y| (0..width).map(move |x| test_flat_cell(x, y)))
                .collect(),
        ));
        let mut grid = OverlayGrid::new_with_retained_wall_plane(width, width);
        grid.seed_neighbor_counts_for_tests(seed);
        sim.overlay_grid = Some(grid);
        for _ in 0..seed {
            sim.resolved_terrain
                .as_ref()
                .unwrap()
                .shared_cell_dummy()
                .adjust_neighbor_count(true);
        }
        sim
    }

    #[test]
    fn native_foot_counter_history_slices() {
        let rows: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/spatial_oracle/foot_neighbors.json"
        ))
        .unwrap();
        assert_eq!(rows.as_array().unwrap().len(), 88);
        for row in rows.as_array().unwrap() {
            let input = &row["input"];
            let seed = input["seed"].as_u64().unwrap_or(0) as u8;
            let mut sim = fixture(128, seed);
            let mut entity = GameEntity::test_default(
                1,
                "ORCA",
                "Americans",
                input["cell"][0].as_u64().unwrap() as u16,
                input["cell"][1].as_u64().unwrap() as u16,
            );
            entity.category = EntityCategory::Aircraft;
            entity.type_ref = sim.intern("ORCA");
            entity.owner = sim.intern("Americans");
            entity.lifecycle.in_limbo = false;
            entity.lifecycle.cell_marked = input["marked"].as_bool().unwrap_or(true);
            entity.position.exact_z_leptons = Some(input["z"].as_i64().unwrap_or(0) as i32);
            entity.navigation.neighbor_state.cell = (
                input["old"][0].as_i64().unwrap() as i16,
                input["old"][1].as_i64().unwrap() as i16,
            );
            sim.substrate.entities.insert(entity);
            let rules = input["rocket_phase"].as_u64().map(|phase| {
                use crate::sim::movement::rocket_movement::{self, RocketPhase};
                rocket_movement::attach_rocket_state(
                    &mut sim.substrate.entities,
                    1,
                    (64, 64),
                    (70, 70),
                    crate::util::fixed_math::SIM_ONE,
                );
                let entity = sim.substrate.entities.get_mut(1).unwrap();
                if phase == 0 {
                    entity.rocket_state = None;
                } else {
                    entity.rocket_state.as_mut().unwrap().phase = [
                        RocketPhase::Ignition,
                        RocketPhase::Tilt,
                        RocketPhase::Ascent,
                        RocketPhase::Cruise,
                        RocketPhase::Terminal,
                        RocketPhase::Secondary,
                    ][phase as usize - 1];
                }
                let key = if input["rocket_rule_offset"] == 0x4E0 {
                    "V3RocketType"
                } else {
                    "DMislType"
                };
                crate::rules::ruleset::RuleSet::from_ini(
                    &crate::rules::ini_parser::IniFile::from_str(&format!(
                        "[General]\n{key}=ORCA\n[AircraftTypes]\n0=ORCA\n[ORCA]\nStrength=100\n"
                    )),
                )
                .unwrap()
            });
            match input["operation"].as_str().unwrap() {
                "unlimbo" => {
                    sim.foot_neighbors_after_unlimbo(1, rules.as_ref());
                }
                "per_cell" => sim.foot_neighbors_at_per_cell(1),
                "limbo" => sim.foot_neighbors_before_limbo(1),
                "owner_change" => sim.foot_neighbors_after_owner_change(1, rules.as_ref()),
                _ => unreachable!(),
            }
            let cell = sim
                .substrate
                .entities
                .get(1)
                .unwrap()
                .navigation
                .neighbor_state
                .cell;
            assert_eq!(
                serde_json::json!([cell.0, cell.1]),
                row["neighbor_cell"],
                "{input}"
            );
            let changed: Vec<_> = sim
                .overlay_grid
                .as_ref()
                .unwrap()
                .retained_neighbor_counts()
                .unwrap()
                .iter()
                .enumerate()
                .filter(|(_, value)| **value != seed)
                .map(|(index, value)| serde_json::json!([index % 128, index / 128, value]))
                .collect();
            assert_eq!(serde_json::json!(changed), row["changed"], "{input}");
            assert_eq!(
                u64::from(
                    sim.resolved_terrain
                        .as_ref()
                        .unwrap()
                        .shared_cell_dummy()
                        .neighbor_count()
                ),
                row["dummy"].as_u64().unwrap(),
                "{input}"
            );
        }
    }

    #[test]
    fn reveal_limbo_and_snapshot_retain_counter_history() {
        let mut sim = fixture(8, 0);
        let mut entity = GameEntity::test_default(1, "MTNK", "Americans", 3, 3);
        entity.type_ref = sim.intern("MTNK");
        entity.owner = sim.intern("Americans");
        sim.substrate.entities.insert(entity);
        sim.substrate.next_stable_object_id = 2;
        sim.reveal(1);
        assert_eq!(
            sim.substrate
                .entities
                .get(1)
                .unwrap()
                .navigation
                .neighbor_state
                .cell,
            (3, 3)
        );
        let before = sim
            .overlay_grid
            .as_ref()
            .unwrap()
            .retained_neighbor_counts()
            .unwrap()
            .to_vec();
        assert_eq!(before.iter().map(|v| u32::from(*v)).sum::<u32>(), 8);
        sim.reveal(1);
        assert_eq!(
            sim.overlay_grid
                .as_ref()
                .unwrap()
                .retained_neighbor_counts()
                .unwrap(),
            before
        );
        // The live position changes in flight without a PerCell callback. Save
        // and load must keep its old source, not reconstruct it from position.
        sim.substrate.entities.get_mut(1).unwrap().position.rx = 5;
        let bytes = GameSnapshot::save(&sim, 0, 0, "neighbor-history", 0);
        let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
        assert_eq!(
            restored
                .substrate
                .entities
                .get(1)
                .unwrap()
                .navigation
                .neighbor_state
                .cell,
            (3, 3)
        );
        assert_eq!(
            restored
                .overlay_grid
                .as_ref()
                .unwrap()
                .retained_neighbor_counts()
                .unwrap(),
            before
        );
        assert_eq!(
            restored
                .overlay_grid
                .as_ref()
                .unwrap()
                .foot_neighbor_revision(),
            0,
            "the replay journal is derived, not serialized"
        );
        // Terrain is rebuilt separately by the production loader. Check the
        // serialized history's hash contribution without equating that missing
        // map/cache state with the live simulation.
        let hash = restored.state_hash();
        restored
            .substrate
            .entities
            .get_mut(1)
            .unwrap()
            .navigation
            .neighbor_state
            .cell = (5, 3);
        assert_ne!(restored.state_hash(), hash);
        restored
            .substrate
            .entities
            .get_mut(1)
            .unwrap()
            .navigation
            .neighbor_state
            .cell = (3, 3);
        restored.resolved_terrain = sim.resolved_terrain.clone();
        restored.techno_limbo(1);
        assert!(
            restored
                .overlay_grid
                .as_ref()
                .unwrap()
                .retained_neighbor_counts()
                .unwrap()
                .iter()
                .all(|v| *v == 0)
        );
        restored.techno_limbo(1);
        assert!(
            restored
                .overlay_grid
                .as_ref()
                .unwrap()
                .retained_neighbor_counts()
                .unwrap()
                .iter()
                .all(|v| *v == 0)
        );
        assert_eq!(
            restored
                .substrate
                .entities
                .get(1)
                .unwrap()
                .navigation
                .neighbor_state
                .cell,
            (3, 3)
        );
    }

    #[test]
    fn native_fly_landing_counter_slices() {
        let rows: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/spatial_oracle/fly_landing_phase.json"
        ))
        .unwrap();
        let mut compared = 0;
        for row in rows.as_array().unwrap() {
            if !row["events"]
                .as_array()
                .unwrap()
                .iter()
                .any(|e| e[0] == "air_remove")
            {
                continue;
            }
            let mut sim = fixture(128, 0);
            let mut entity = GameEntity::test_default(1, "ORCA", "Americans", 64, 64);
            let old = &row["before"]["neighbor_cell"];
            entity.navigation.neighbor_state.cell = (
                old[0].as_i64().unwrap() as i16,
                old[1].as_i64().unwrap() as i16,
            );
            sim.substrate.entities.insert(entity);
            let seed = row["input"]["neighbor_seed"].as_u64().unwrap_or(0) as u8;
            sim.overlay_grid
                .as_mut()
                .unwrap()
                .seed_neighbor_counts_for_tests(seed);
            sim.foot_neighbors_after_fly_landing(1);
            let plane = sim
                .overlay_grid
                .as_ref()
                .unwrap()
                .retained_neighbor_counts()
                .unwrap();
            for cell in row["after"]["neighbors"].as_array().unwrap() {
                let index =
                    cell[1].as_u64().unwrap() as usize * 128 + cell[0].as_u64().unwrap() as usize;
                assert_eq!(
                    u64::from(plane[index]),
                    cell[2].as_u64().unwrap(),
                    "{}",
                    row["input"]
                );
            }
            compared += 1;
        }
        assert!(
            compared >= 12,
            "landed counter slices only; full Rust landing remains separate"
        );
    }

    #[test]
    fn journal_lag_and_revision_wrap_force_rebuild() {
        let mut journal = NeighborCountChanges::default();
        assert_eq!(journal.since(0).unwrap().count(), 0);
        for n in 0..4097 {
            assert!(journal.record(n, n % 2 == 0));
        }
        assert!(journal.since(0).is_none());
        assert_eq!(journal.since(1).unwrap().count(), 4096);
        assert_eq!(
            journal.since(4096).unwrap().collect::<Vec<_>>(),
            vec![(4096, 1)]
        );
        journal.revision = u64::MAX;
        assert!(!journal.record(3, false));
        assert!(journal.since(u64::MAX).is_none());
        assert_eq!(
            journal.since(0).unwrap().collect::<Vec<_>>(),
            vec![(3, 255)]
        );
    }
}
