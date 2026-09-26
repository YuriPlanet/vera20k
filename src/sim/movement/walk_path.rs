//! Walk75AEC0's no-queue path request and its Infantry receivers.
//!
//! The caller arms its timer (0x75AF69..0x75AF8F) and enters the shared
//! `FootClass::Find_Path` owner (`foot_path.rs`). A failed search reaches the
//! Infantry receiver `+0x500` (0x0051DAF0) inside Find_Path, then the Walk
//! continuation 0x75AFD3 here. Original caller comparisons:
//! tools/spatial_oracle/walk_failed_path.

use super::block_index::LentOwnerBlockSet;
use super::foot_path::{FindPathResult, coord_cell};
use super::ground_pose;
use super::infantry_entry::InfantryEntryArgs;
use super::movement_tick::FootPathRequest;
use crate::map::overlay_types::OverlayTypeRegistry;
use crate::map::resolved_terrain::NativeCellQuery;
use crate::rules::ruleset::RuleSet;
use crate::sim::components::DriveCoord;
use crate::sim::pathfinding::PathGrid;
use crate::sim::world::Simulation;

/// Infantry 0x51DAF6..0x51DB44: the action the failed-path receiver requests
/// from `Do_Action` before it tests the current cell.
pub(crate) fn failed_path_requested_action(doing: i32, prone: bool) -> i32 {
    if (27..=30).contains(&doing) {
        28
    } else if prone {
        2
    } else {
        0
    }
}

impl Simulation {
    /// Synchronous75AFC5 -> Foot4D3920. A successful result resumes this same
    /// Process invocation; a failed result owns its cleanup and must never
    /// enter the arrival finalizer (which would snap the actor's exact XYZ).
    pub(crate) fn run_walk_path_request(
        &mut self,
        request: &FootPathRequest,
        lent: Option<&mut LentOwnerBlockSet>,
        rules: Option<&RuleSet>,
        fallback: Option<&PathGrid>,
        registry: Option<&OverlayTypeRegistry>,
    ) -> Result<bool, String> {
        let rules = rules.ok_or("Walk path request requires rules")?;
        let id = request.entity_id;
        let frame = self.session.binary_frame;
        //75AF69..8F arms the caller timer before Foot4D3920.
        self.substrate
            .entities
            .get_mut(id)
            .ok_or("retired Walk path requester")?
            .navigation
            .path_runtime
            .start_movement(frame, rules.general.path_delay_ticks());
        match self.foot_find_path(request, lent, rules, fallback, registry)? {
            FindPathResult::Route => {
                //75B2DF..E2 is the success caller's retry reset. Existing
                //head production resumes at its ordinary shared owner.
                self.substrate
                    .entities
                    .get_mut(id)
                    .ok_or("retired Walk path requester")?
                    .navigation
                    .path_runtime
                    .retries_left = super::PATH_STUCK_INIT;
                Ok(true)
            }
            FindPathResult::EmptyRoute => {
                //A zero-cost route (the goal is the mover's own Cell) copies
                //no word. 75B2DF..75B2F9 then runs the success arm: the retry
                //reset, and Infantry vt+4F8 (521EB0) answers false without
                //JumpJet=. 75B2FF..75B5A7 reads the untouched Foot+5E0
                //terminator and steps toward (-1 & 7) = octant 7; the queue
                //head stays -1. Evidence: instruction reading only.
                let actor = self
                    .substrate
                    .entities
                    .get_mut(id)
                    .ok_or("retired Walk path requester")?;
                let current = (actor.position.rx, actor.position.ry);
                let (dx, dy) = crate::util::direction::DIRECTION_DELTAS[7];
                let next = (
                    current.0.wrapping_add_signed(dx as i16),
                    current.1.wrapping_add_signed(dy as i16),
                );
                let layer = if actor.on_bridge {
                    super::locomotor::MovementLayer::Bridge
                } else {
                    super::locomotor::MovementLayer::Ground
                };
                request.install_route(actor, vec![current, next], vec![layer, layer]);
                actor.navigation.path_replay.clear_live_head();
                actor.navigation.path_runtime.retries_left = super::PATH_STUCK_INIT;
                Ok(true)
            }
            FindPathResult::Failed => {
                //75AFD3: after a precheck refusal (no receiver) or a core
                //failure (the Infantry receiver already Stopped Walk), the
                //locomotor's own zone recheck reads the retained destination.
                self.finish_failed_walk_process(id, rules, registry)?;
                Ok(false)
            }
        }
    }

    /// `InfantryClass::Stop_Driver`, Infantry vtable +0x500 = 0x0051DAF0,
    /// reached from the `Find_Path` failure at 0x4D4044, the Stun
    /// (`FootClass::Stun @ 0x004D5660`), the kill's Infantry arm
    /// (`0x005180FE`) and `Do_Action` 0x51D6F0 at zero health. Order:
    /// `Do_Action` request by Doing and the prone byte (+6DB), current-cell
    /// `Can_Enter_Cell` (+1AC) with the facing octant and the Techno height
    /// helper 0x5F5F00 (this+8C OnBridge plus the current cell's +11B level
    /// through vtable +0x1BC), the +6DC answer byte, then Foot 0x4D55C0 ->
    /// locomotor +0x48: Walk Stop 0x75ADA0, or the Jumpjet's `Stop_Moving`
    /// 0x0054B4D0. Another locomotor's Stop is not ported: no stock
    /// infantryman has one.
    pub(crate) fn infantry_stop_driver(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) -> Result<(), String> {
        let actor = self
            .substrate
            .entities
            .get(id)
            .ok_or("retired failed-path receiver")?;
        let object = self
            .object_type(actor.type_ref(), rules)
            .ok_or("failed-path receiver requires the Infantry type")?;
        let facts = super::infantry_action::DoActionType {
            type_id: object.id.clone(),
            movement_zone: object.movement_zone,
            crawls: object.crawls,
        };
        let doing = actor
            .mission_leaf
            .as_infantry()
            .ok_or("failed-path receiver requires Infantry Doing")?
            .doing();
        let prone = actor
            .infantry
            .as_ref()
            .is_some_and(|infantry| infantry.is_prone);
        let on_bridge = actor.on_bridge;
        let coord = ground_pose::position_world_coord(&actor.position);
        //0x51DB68..0x51DB7A: the 16-bit facing (+388) becomes an octant through
        //`((facing >> 12) + 1) >> 1 & 7`; the stored 8-bit facing is its high byte.
        let direction = (((i32::from(actor.facing) >> 4) + 1) >> 1) & 7;
        let requested = failed_path_requested_action(doing, prone);
        self.apply_infantry_do_action(id, requested, false, &facts, rules)?;

        let terrain = self
            .resolved_terrain
            .as_ref()
            .ok_or("failed-path receiver requires map cells")?;
        let cells = NativeCellQuery::canonical(terrain);
        let cell = cells.lookup(coord_cell(coord));
        //0x5F5F00 (ECX = this Infantry, 0x51DB78): the current cell's signed
        //level byte (+11B via vtable +1BC) plus four when OnBridge (+8C).
        let height = ground_pose::query_object_cell_height(&cells, coord, on_bridge);
        // A Can_Enter_Cell input VERA lacks (a terrain without the owner's
        // speed row) leaves the +6DC byte as it was; the Stop below still runs,
        // as the native call always reaches it.
        let answer = self.infantry_can_enter(
            id,
            cell,
            InfantryEntryArgs {
                direction,
                height,
                previous_cell: None,
            },
            rules,
            registry,
        );
        let actor = self
            .substrate
            .entities
            .get_mut(id)
            .ok_or("failed-path receiver actor retired during Can_Enter_Cell")?;
        match answer {
            //0x51DBAC stores the zero answer byte; 0x51DBBE stores 1 for any other.
            Ok(answer) => {
                actor
                    .infantry
                    .as_mut()
                    .ok_or("failed-path receiver requires Infantry runtime state")?
                    .cell_entry_blocked = answer.is_nonzero();
            }
            Err(cause) => log::debug!("infantry {id} Stop_Driver Can_Enter_Cell: {cause}"),
        }
        //0x4D55C0 -> ILocomotion +0x48. Walk 0x75ADA0 clears the destination
        //and, with no paid head, the IsMoving byte; the Jumpjet's re-targets
        //the nearest passable cell (a Scenario draw for Infantry placement).
        let locomotor = actor
            .locomotor
            .as_mut()
            .ok_or("Stop_Driver requires a locomotor")?;
        if locomotor.jumpjet_runtime().is_none() {
            locomotor.stop_walk();
        } else {
            self.jumpjet_stop_moving(id, Some(rules), registry);
        }
        Ok(())
    }

    fn finish_failed_walk_process(
        &mut self,
        id: u64,
        rules: &RuleSet,
        _registry: Option<&OverlayTypeRegistry>,
    ) -> Result<(), String> {
        //ESI points into the live Walk destination, not a copy retained before
        //FindPath. Infantry+500 can have cleared it, and House can replace it.
        let destination = self
            .substrate
            .entities
            .get(id)
            .and_then(|e| e.locomotor.as_ref())
            .and_then(|l| l.walk_destination())
            .unwrap_or(DriveCoord { x: 0, y: 0, z: 0 });
        if !self.foot_path_zone_precheck(id, destination, rules)? {
            self.set_walk_null_destination(id, Some(rules));
        } else {
            //Only the nonhuman relocation (0x4D41C2) leaves a live destination
            //after the receiver's Stop, and that arm stops earlier.
            return Err(
                "Walk failed-search retained-zone continuation awaits the nonhuman relocation owner"
                    .into(),
            );
        }
        let actor = self
            .substrate
            .entities
            .get_mut(id)
            .ok_or("retired failed Walk actor")?;
        if actor
            .locomotor
            .as_ref()
            .is_some_and(|l| l.walk_destination().is_none() && l.step_head().is_none())
        {
            actor.movement_target = None;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn receiver_action_follows_0x51daf6_selection() {
        for doing in 27..=30 {
            assert_eq!(failed_path_requested_action(doing, true), 28);
        }
        assert_eq!(failed_path_requested_action(0, true), 2);
        assert_eq!(failed_path_requested_action(-1, false), 0);
        assert_eq!(failed_path_requested_action(3, false), 0);
    }
}
