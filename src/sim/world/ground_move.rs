//! Ground move orders: the path-search inputs an order's move reads.
//!
//! Pursuing and resumed orders, miners, ejected passengers, the factory rally
//! and command routes without a direct class dispatch give a ground mover its
//! destination through
//! [`Simulation::issue_ground_move`]: the blocker plane and, where the site
//! asks for them, the mover's owner block sets, then
//! `issue_move_command_with_destination`. Both come from the products the
//! movement pass keeps current through the entity touch log
//! (`MovementPassCache`). Each equals a whole-world build from the entities,
//! terrain, overlays, alliances and rules at this point of the frame; debug
//! builds compare the two on every read. Scatter, air and other direct moves
//! do not come through here. Ordinary represented Unit Move dispatches directly
//! through the concrete destination dispatcher in `world_commands`. Internal
//! Drive orders take the same Unit setter here before acquiring path inputs.
//! A Walk infantryman takes the existing Infantry
//! destination owner, including repeated and queued requests; its first
//! Process owns the path search. A mover on Teleport takes
//! its class setter instead of a route: no pass moves a Teleport owner. A
//! Jumpjet's cell order likewise takes Foot's setter, whose `Move_To` flies
//! it; an object order still takes the route, and the Jumpjet's `Process`
//! hands its goal to `Move_To` (`apply_jumpjet_adapter_order`).
//!
//! Residual, carried over unchanged: the sites differ in what they hand the
//! search, with no recorded native reason. The resumed-order, both miner and
//! the factory-rally moves search without owner block sets; the resumed-order
//! and idle-miner moves also search without terrain costs. Only a mover that
//! searches when the order is given reads these inputs: not Drive, Ship or a
//! represented Walk infantryman, which accept first and search in
//! their own turn. Aligning the sites changes paths and needs native evidence
//! per site.

use super::Simulation;
use crate::map::entities::EntityCategory;
use crate::map::overlay_types::OverlayTypeRegistry;
use crate::rules::locomotor_type::{LocomotorKind, SpeedType};
use crate::rules::ruleset::RuleSet;
use crate::sim::components::{DriveCoord, NavTargetRef};
use crate::sim::movement::{self, DestinationTiming};
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
    /// An object order's reference and coordinate for the route adapter.
    /// A represented class owner instead re-reads the live target's +4C.
    pub(crate) object_destination: Option<(NavTargetRef, DriveCoord)>,
}

impl Simulation {
    /// Issue `order` with the kept block sets and blocker plane, against the
    /// canonical path grid. For a represented class, true means dispatched,
    /// including a guarded no-op: its native setter is void. Route adapters
    /// require a published grid. A mover on
    /// Teleport takes its class setter ([`Self::teleport_destination`]), and
    /// a Jumpjet's cell order takes Foot's
    /// ([`Self::jumpjet_cell_destination`]), which ignores `queue`. A Walk
    /// infantryman takes Infantry51AA40 before borrowing any path-search
    /// inputs. Its class flag1 never appends to NavQueue. A represented Drive
    /// Unit likewise takes Unit741970 before any grid read. Its same-Nav/force,
    /// deployment, radio and queue decisions belong to that sole class owner.
    pub(crate) fn issue_ground_move(
        &mut self,
        order: GroundMove,
        rules: Option<&RuleSet>,
        registry: Option<&OverlayTypeRegistry>,
    ) -> bool {
        if let Some(accepted) =
            self.teleport_destination(order.entity_id, order.target, rules, registry)
        {
            return accepted;
        }
        if let Some(rules) = rules {
            let requested = order.object_destination.map_or_else(
                || NavTargetRef::cell(order.target.0, order.target.1),
                |(reference, _)| reference,
            );
            if self.drive_unit_setter_receiver(order.entity_id) {
                // Event4C747C dispatches the void Unit741970(target,1),
                // including its unchanged-NavCom return741A80..741A9C.
                // The represented dispatcher preserves that return contract
                // and re-reads an object's live +4C through the class owner.
                // Native boundary controls: track_destination.json, Drive
                // Unit rows. Ship's naval-rally caller still prewrites NavCom;
                // its class/caller migration requires legal naval execution
                // (remaining #687/#689), so it keeps the adapter below.
                return self
                    .assign_destination_represented(
                        order.entity_id,
                        Some(requested),
                        Some(rules),
                        registry,
                    )
                    .map_or_else(
                        |cause| {
                            log::warn!("Unit ground destination {}: {cause}", order.entity_id);
                            false
                        },
                        |()| true,
                    );
            }
            if self.infantry_setter_receiver(order.entity_id, requested, rules) {
                // Event4C747C and the other concrete destination callers
                // dispatch class+480(target,1), not a second Walk composition.
                // The class owns current-cell admission, Stop_Driver, prone
                // Up and path clearing before the shared Foot/Walk tail.
                return self
                    .assign_infantry_walk_destination(
                        order.entity_id,
                        requested,
                        order.speed,
                        rules,
                        registry,
                    )
                    .unwrap_or_else(|cause| {
                        log::warn!("Infantry ground destination {}: {cause}", order.entity_id);
                        false
                    });
            }
        }
        if order.object_destination.is_none()
            && let Some(accepted) =
                self.jumpjet_cell_destination(order.entity_id, order.target, order.speed, rules)
        {
            return accepted;
        }
        let Some(grid) = self.path_grid_snapshot() else {
            return false;
        };
        let grid = grid.as_ref();
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

    /// `vt+0x480(cell, 1)` for a mover whose active locomotor is Teleport: its
    /// class setter, whose Foot tail reaches Teleport Move_To (`0x00718100`).
    /// The Teleport Process warps; it follows no route.
    /// - Infantry: the Infantry setter (`0x0051AA40`), which never reads
    ///   `Teleporter=`.
    /// - A Unit: the Unit setter (`0x00741970`). A `Teleporter=` one (the
    ///   Chrono Miner) takes its Teleporter arm, which drives it except onto a
    ///   dock; the others on Teleport are CMON and SMON (`UnloadingClass=`
    ///   harvesters) and the Chrono Warp's.
    ///
    /// RESIDUAL: the order's queue flag and object destination are not
    /// carried: a queued waypoint replaces the order, and an object order
    /// warps to the object's cell. Trigger: a queued or object order to a
    /// Chrono unit. Frequency: uncommon.
    pub(crate) fn teleport_destination(
        &mut self,
        id: u64,
        cell: (u16, u16),
        rules: Option<&RuleSet>,
        registry: Option<&OverlayTypeRegistry>,
    ) -> Option<bool> {
        let entity = self.substrate.entities.get(id)?;
        if entity.locomotor.as_ref()?.active_kind() != LocomotorKind::Teleport {
            return None;
        }
        let category = entity.category;
        let Some(rules) = rules else {
            return Some(false);
        };
        Some(match category {
            EntityCategory::Infantry => self
                .set_infantry_destination(id, NavTargetRef::cell(cell.0, cell.1), rules, registry)
                .unwrap_or_else(|error| {
                    log::debug!("Teleport infantry order {id} refused: {error}");
                    false
                }),
            EntityCategory::Unit => {
                self.set_unit_destination(id, NavTargetRef::cell(cell.0, cell.1), rules, true)
            }
            EntityCategory::Aircraft | EntityCategory::Structure => return None,
        })
    }
}
