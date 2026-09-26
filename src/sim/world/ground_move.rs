//! The one path that gives an order's ground mover its destination.
//!
//! Commands, resumed and pursuing orders, miners and ejected passengers all
//! issue the same move: the owner's friendly-passable block sets and the
//! blocker plane, then `issue_move_command_with_destination`. Both inputs are
//! pure functions of the entities, so they come from the kept products
//! (`MovementPassCache`) the movement pass already maintains through the
//! entity touch log. They equal a whole-world build at this point of the
//! frame; debug builds compare the two on every read.

use super::Simulation;
use crate::map::overlay_types::OverlayTypeRegistry;
use crate::rules::locomotor_type::SpeedType;
use crate::rules::ruleset::RuleSet;
use crate::sim::components::{DriveCoord, NavTargetRef};
use crate::sim::movement::{self, DestinationTiming};
use crate::sim::pathfinding::PathGrid;
use crate::util::fixed_math::SimFixed;

/// One ground move order.
pub(crate) struct GroundMove {
    pub(crate) entity_id: u64,
    pub(crate) target: (u16, u16),
    pub(crate) speed: SimFixed,
    pub(crate) queue: bool,
    /// Terrain costs for this speed type; `None` searches without them.
    pub(crate) speed_type: Option<SpeedType>,
    /// Whether the search reads the block sets as the mover's owner sees
    /// them (friendly movers passable); otherwise it reads none.
    pub(crate) owner_blocks: bool,
    /// An object order's captured coordinate (see
    /// `issue_move_command_with_destination`).
    pub(crate) object_destination: Option<(NavTargetRef, DriveCoord)>,
}

impl Simulation {
    /// Issue `order` with the kept block sets and blocker plane. Returns
    /// whether the mover accepted the destination.
    pub(crate) fn issue_ground_move(
        &mut self,
        grid: &PathGrid,
        order: GroundMove,
        overlay_registry: Option<&OverlayTypeRegistry>,
        rules: Option<&RuleSet>,
    ) -> bool {
        let block_owner = order
            .owner_blocks
            .then(|| self.substrate.entities.get(order.entity_id))
            .flatten()
            .map(|entity| entity.owner());
        let lent = block_owner.map(|owner| {
            let sets = self.movement_pass_cache.lend_block_set(
                owner,
                &mut self.substrate.entities,
                &self.house_alliances,
                &self.interner,
                rules,
            );
            (owner, sets)
        });
        let blocker_neighbor_counts = self.movement_pass_cache.blocker_plane(
            &mut self.substrate.entities,
            grid,
            self.resolved_terrain.as_ref(),
            self.overlay_grid.as_ref(),
            overlay_registry,
            &self.interner,
            rules,
        );
        let issued = movement::issue_move_command_with_destination(
            &mut self.substrate.entities,
            grid,
            order.entity_id,
            order.target,
            order.speed,
            order.queue,
            order
                .speed_type
                .and_then(|speed_type| self.terrain_costs.get(&speed_type)),
            lent.as_ref().map(|(_, lent)| &lent.sets.0),
            self.resolved_terrain.as_ref(),
            self.zone_grid.as_ref(),
            lent.as_ref().map(|(_, lent)| &lent.sets.1),
            Some(blocker_neighbor_counts),
            self.playfield_bounds,
            Some(&mut self.substrate.cell_occupation),
            order.object_destination,
            DestinationTiming::from_rules(self.session.binary_frame, rules),
        );
        if let Some((owner, lent)) = lent {
            self.movement_pass_cache.give_back(owner, lent);
        }
        issued
    }
}
