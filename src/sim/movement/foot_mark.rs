//! Shared Foot4D3780 -> Object5F5850 mark/list corridor for ground movement.
//! Walk and Drive/Ship differ in their raw occupation receiver, not in the
//! lifetime of Cell.Marked or the ordering of list insertion and Recalc.
//! AddContent47E8A0 discovery/tag4 is still a receiver gap at the put callback.

use super::{ground_pose, locomotor::MovementLayer};
use crate::map::entities::EntityCategory;
use crate::map::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::occupancy::CellListInsertion;
use crate::sim::pathfinding::PathGrid;
use crate::sim::world::Simulation;

impl Simulation {
    /// Object5F5850 publishes the mark byte before Foot Exit/Enter. REMOVE
    /// retains the old addressed cell even if a later receiver changes XYZ.
    pub(super) fn foot_mark_remove(
        &mut self,
        id: u64,
        rules: Option<&RuleSet>,
        fallback: Option<&PathGrid>,
        registry: Option<&OverlayTypeRegistry>,
    ) {
        let removed = self.substrate.entities.get_mut(id).and_then(|entity| {
            if entity.lifecycle.in_limbo || !entity.lifecycle.cell_marked {
                return None;
            }
            entity.lifecycle.cell_marked = false;
            Some((
                (entity.position.rx, entity.position.ry),
                if entity.on_bridge {
                    MovementLayer::Bridge
                } else {
                    MovementLayer::Ground
                },
            ))
        });
        if let Some((cell, layer)) = removed {
            self.substrate
                .occupancy
                .remove_on_layer(cell.0, cell.1, id, layer);
            // Foot enable and concrete raw receiver are live after unlink.
            self.foot_mark_raw(id, false, fallback);
            self.recalculate_track_cell(cell, rules, registry);
        }
    }

    pub(super) fn foot_mark_put(
        &mut self,
        id: u64,
        rules: Option<&RuleSet>,
        fallback: Option<&PathGrid>,
        registry: Option<&OverlayTypeRegistry>,
    ) {
        self.foot_mark_put_observed(id, rules, fallback, registry, &mut |_, _| {});
    }

    /// The receiver boundary is after the marked byte and list link, before
    /// raw occupation. Both Foot enable and XYZ reload after that receiver;
    /// Recalc still addresses the cell retained by this Enter invocation.
    pub(super) fn foot_mark_put_observed(
        &mut self,
        id: u64,
        rules: Option<&RuleSet>,
        fallback: Option<&PathGrid>,
        registry: Option<&OverlayTypeRegistry>,
        receive: &mut impl FnMut(&mut Simulation, u64),
    ) {
        let entered = self.substrate.entities.get_mut(id).and_then(|entity| {
            if entity.lifecycle.in_limbo || entity.lifecycle.cell_marked {
                return None;
            }
            entity.lifecycle.cell_marked = true;
            entity.occupancy_enter_order = self.substrate.next_occupancy_enter_order.next();
            let cell = (entity.position.rx, entity.position.ry);
            self.substrate.occupancy.add(
                cell.0,
                cell.1,
                id,
                if entity.on_bridge {
                    MovementLayer::Bridge
                } else {
                    MovementLayer::Ground
                },
                entity.sub_cell,
                CellListInsertion::from_category(entity.category),
            );
            Some(cell)
        });
        if let Some(cell) = entered {
            receive(self, id);
            self.foot_mark_raw(id, true, fallback);
            self.recalculate_track_cell(cell, rules, registry);
        }
    }

    fn foot_mark_raw(&mut self, id: u64, put: bool, fallback: Option<&PathGrid>) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        if !entity.foot_occupation_enabled {
            return;
        }
        match entity.category {
            EntityCategory::Unit => self.track_raw_mark(id, put, fallback),
            EntityCategory::Infantry => {
                let owner = entity.owner();
                let coord = ground_pose::position_world_coord(&entity.position);
                super::walk_head::raw_at(
                    &mut self.substrate.raw_cell_occupation,
                    owner,
                    coord,
                    put,
                    self.resolved_terrain.as_ref(),
                    self.path_grid.as_deref().or(fallback),
                );
            }
            // The ground movement callers above are Unit/Infantry owners.
            // Other concrete Foot raw receivers are not supplied by this host.
            _ => {}
        }
    }
}

#[cfg(test)]
#[path = "foot_mark_tests.rs"]
mod tests;
