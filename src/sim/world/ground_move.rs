//! Ground move orders: the path-search inputs an order's move reads.
//!
//! Command orders, pursuing and resumed orders, miners, ejected passengers
//! and the factory rally give a ground mover its destination through
//! [`Simulation::issue_ground_move`]: the blocker plane and, where the site
//! asks for them, the mover's owner block sets, then
//! `issue_move_command_with_destination`. Both come from the products the
//! movement pass keeps current through the entity touch log
//! (`MovementPassCache`). Each equals a whole-world build from the entities,
//! terrain, overlays, alliances and rules at this point of the frame; debug
//! builds compare the two on every read. Scatter, teleport, air and other
//! direct moves do not come through here.
//!
//! Residual, carried over unchanged: the sites differ in what they hand the
//! search, with no recorded native reason. The resumed-order, both miner and
//! the factory-rally moves search without owner block sets; the resumed-order
//! and idle-miner moves also search without terrain costs. Only a mover that
//! searches when the order is given reads these inputs: not Drive, Ship or a
//! Walk mover taking a fresh destination, which accept first and search in
//! their own turn. Aligning the sites changes paths and needs native evidence
//! per site.

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
