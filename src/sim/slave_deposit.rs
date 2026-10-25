//! Shared SlaveManager6B0880 query used by Infantry51C2BC and Walk75C4B0.
//! Reads the slave's SlaveOwner (+2DC) and its master's manager (+2D8); no
//! harvest-state gate.
//! Ordered map lookups retain real/dummy Cell identity. This is a spatial
//! predicate, not a replacement for slave harvesting or manager AI.
//! Evidence: docs/research/bridges/05-damage-collapse-repair-cabhut/
//! LIVE_REPAIR_SLAVE_PREDICATE.md (original bodies, callers and retail limits).
use crate::map::{
    cell_index::NativeCellIdentity as Cell, entities::EntityCategory,
    resolved_terrain::ResolvedTerrainGrid,
};
use crate::rules::ruleset::RuleSet;
use crate::sim::movement::{ground_pose, locomotor::MovementLayer};
use crate::sim::{entity_store::EntityStore, intern::StringInterner, occupancy::OccupancyGrid};

pub(crate) struct SlaveDepositQuery<'a> {
    pub entities: &'a EntityStore,
    pub occupancy: &'a OccupancyGrid,
    pub terrain: &'a ResolvedTerrainGrid,
    pub rules: &'a RuleSet,
    pub interner: &'a StringInterner,
}

impl SlaveDepositQuery<'_> {
    /// The caller tests both child+2DC and master+2D8 before6B0880. An
    /// existing empty manager remains admitted to the geometry/lookup body.
    pub(crate) fn master(&self, slave: u64) -> Option<u64> {
        let master = self.entities.get(slave)?.slave.owner()?;
        self.entities
            .get(master)?
            .slave_manager
            .is_some()
            .then_some(master)
    }

    fn first_building(&self, cell: Cell) -> Option<u64> {
        let Cell::Real(index) = cell else {
            return None;
        };
        let c = &self.terrain.cells()[index];
        self.occupancy
            .first_building_on_layer(c.rx, c.ry, MovementLayer::Ground)
    }

    /// Native6B0690/6B0880 use current master+1B8 and Type foundation(false).
    /// The first secondary lookup precedes the primary lookup, even when the
    /// primary will match. A second secondary lookup is conditional afterward.
    pub(crate) fn admits(&self, slave: u64, master: u64, queried: Cell) -> bool {
        if self.master(slave) != Some(master) {
            return false;
        }
        let Some(owner) = self.entities.get(master) else {
            return false;
        };
        let building = owner.category() == EntityCategory::Structure;
        let foundation = self
            .interner
            .object_type(owner.type_ref(), self.rules)
            .map(|kind| kind.foundation.dimensions());
        let Some(primary) = crate::sim::slave_manager::deploy_center(owner, foundation) else {
            return false;
        };
        let secondary = if building {
            //6B0934 recomputes6B0690; no callback changes its inputs here.
            let candidate = (primary.0, primary.1.wrapping_sub(1));
            let cell = self.terrain.native_cell_identity(candidate);
            (self.first_building(cell) == Some(master)).then_some(candidate)
        } else {
            None
        };
        let primary_cell = self.terrain.native_cell_identity(primary);
        let matched = queried == primary_cell
            || secondary
                .filter(|&c| c != (0, 0))
                .is_some_and(|c| self.terrain.native_cell_identity(c) == queried);
        //6B09E0 only touches the manager list after geometry has matched.
        matched
            && owner
                .slave_manager
                .as_ref()
                .is_some_and(|manager| manager.holds(slave))
    }

    /// Walk75C46A ->47C3D0(0,0,ground,null), then repeated input lookup at
    ///75C49A before6B0880. Raw clear is owned by the caller before entry here.
    pub(crate) fn walk_priority(
        &self,
        slave: u64,
        input: crate::sim::components::DriveCoord,
    ) -> bool {
        let Some(master) = self.master(slave) else {
            return false;
        };
        let coord = ((input.x / 256) as i16, (input.y / 256) as i16);
        let selected = self.terrain.native_cell_identity(coord);
        let nearest = match selected {
            Cell::Dummy => None,
            Cell::Real(index) => {
                let c = &self.terrain.cells()[index];
                let candidates = self
                    .occupancy
                    .get(c.rx, c.ry)
                    .into_iter()
                    .flat_map(|list| list.iter_layer(MovementLayer::Ground))
                    .filter_map(|entry| self.entities.get(entry.entity_id))
                    .map(|entity| {
                        let [x, y] = ground_pose::object_center_xy(entity);
                        (
                            entity.stable_id(),
                            true,
                            crate::sim::cell_kernel::CellQueryPoint { x, y },
                        )
                    });
                crate::sim::cell_kernel::nearest_eligible_in_order(
                    crate::sim::cell_kernel::CellQueryPoint { x: 0, y: 0 },
                    candidates,
                )
            }
        };
        if nearest != Some(master) {
            return false;
        }
        let selected = self.terrain.native_cell_identity(coord);
        self.admits(slave, master, selected)
    }
}
