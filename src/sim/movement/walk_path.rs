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
use crate::rules::locomotor_type::MovementZone;
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

/// `Do_Action` 0x0051D6F0 admission for the three receiver requests, after the
/// requested-sequence gate: an unchanged action refuses (0x51D90B), and an
/// established non-idle action refuses unless its record byte0 is
/// interruptible (0x51D934). `-1` always admits.
pub(crate) fn failed_path_do_action_admits(current: i32, requested: i32) -> bool {
    if requested == current {
        return false;
    }
    current == -1
        || crate::rules::infantry_sequence::action_record(current)
            .is_some_and(|record| record.interruptible)
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
            FindPathResult::EmptyRoute => Err(
                "Walk core returned an unclassified zero-cost path; native +4 is cost, not count"
                    .into(),
            ),
            FindPathResult::Failed => {
                //75AFD3: after a precheck refusal (no receiver) or a core
                //failure (the Infantry receiver already Stopped Walk), the
                //locomotor's own zone recheck reads the retained destination.
                self.finish_failed_walk_process(id, rules, registry)?;
                Ok(false)
            }
        }
    }

    /// Infantry vtable +0x500 = 0x0051DAF0, reached from the `Find_Path`
    /// failure at 0x4D4044 (and from `Do_Action` 0x51D6F0 at zero health,
    /// which no live actor here has). Order: `Do_Action` request by Doing and
    /// the prone byte (+6DB), current-cell `Can_Enter_Cell` (+1AC) with the
    /// facing octant and the Techno height helper 0x5F5F00 (this+8C OnBridge
    /// plus the current cell's +11B level through vtable +0x1BC), the +6DC
    /// answer byte, then Foot 0x4D55C0 -> locomotor +0x48 (Walk Stop 0x75ADA0).
    pub(crate) fn run_infantry_failed_path_receiver(
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
        let type_id = object.id.clone();
        let movement_zone = object.movement_zone;
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
        self.apply_failed_path_do_action(
            id,
            doing,
            requested,
            &type_id,
            movement_zone,
            on_bridge,
            rules,
        )?;

        let terrain = self
            .resolved_terrain
            .as_ref()
            .ok_or("failed-path receiver requires map cells")?;
        let cells = NativeCellQuery::canonical(terrain);
        let cell = cells.lookup(coord_cell(coord));
        //0x5F5F00 (ECX = this Infantry, 0x51DB78): the current cell's signed
        //level byte (+11B via vtable +1BC) plus four when OnBridge (+8C).
        let height = ground_pose::query_object_cell_height(&cells, coord, on_bridge);
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
        )?;
        let actor = self
            .substrate
            .entities
            .get_mut(id)
            .ok_or("failed-path receiver actor retired during Can_Enter_Cell")?;
        //0x51DBAC stores the zero answer byte; 0x51DBBE stores 1 for any other.
        actor
            .infantry
            .as_mut()
            .ok_or("failed-path receiver requires Infantry runtime state")?
            .cell_entry_blocked = answer.is_nonzero();
        //0x4D55C0 -> ILocomotion +0x48. Walk 0x75ADA0 clears the destination
        //and, with no paid head, the IsMoving byte.
        actor
            .locomotor
            .as_mut()
            .ok_or("failed-path receiver requires the Walk locomotor")?
            .stop_walk();
        Ok(())
    }

    /// Bounded `InfantryClass::Do_Action` 0x0051D6F0 for the receiver's
    /// requests 0, 2 and 28 with force and random-frame arguments 0. The
    /// requested-sequence count gate (Type+E3C record) precedes everything.
    /// Two remaps change the written action and carry side effects Rust does
    /// not own (the +6E8 wet reclassification with its sound request at
    /// 0x51D842..0x51D8B8, and the airborne Hover remap through vtable+0x54):
    /// an AmphibiousDestroyer type on a Water/Beach cell off a bridge, or a
    /// request 0 while airborne. Both leave Doing untouched here; Doing is
    /// otherwise not maintained by an InfantryClass::AI port and only its
    /// sim consumers (readiness, hut Scatter, the Walk null setter) read it.
    /// Frame (+F8), logical timer (+100..+10C) and image frame (+3E) have no
    /// Rust owner and stay a visual residual.
    #[allow(clippy::too_many_arguments)]
    fn apply_failed_path_do_action(
        &mut self,
        id: u64,
        current: i32,
        requested: i32,
        type_id: &str,
        movement_zone: MovementZone,
        on_bridge: bool,
        rules: &RuleSet,
    ) -> Result<(), String> {
        use crate::rules::animation_sequence::SequenceKind;
        let kind = match requested {
            0 => SequenceKind::Stand,
            2 => SequenceKind::Prone,
            28 => SequenceKind::Deployed,
            other => {
                return Err(format!(
                    "failed-path receiver requested action {other} outside 0x51DAF6..0x51DB44"
                ));
            }
        };
        //0x51D70F: a zero requested-sequence count refuses before any state.
        let has_sequence = rules
            .animation_sequence(type_id)
            .and_then(|set| set.get(&kind))
            .is_some_and(|sequence| sequence.frame_count != 0);
        if !has_sequence {
            return Ok(());
        }
        if movement_zone == MovementZone::AmphibiousDestroyer && !on_bridge {
            let actor = self
                .substrate
                .entities
                .get(id)
                .ok_or("retired Do_Action receiver")?;
            let land = self
                .resolved_terrain
                .as_ref()
                .and_then(|terrain| terrain.cell(actor.position.rx, actor.position.ry))
                .map(|cell| cell.yr_cell_land_type);
            //0x51D7C6: LandType Water(2)/Beach(6) remaps 0/2 -> 16 and writes +6E8.
            if matches!(land, Some(2) | Some(6)) {
                return Ok(());
            }
        }
        if !failed_path_do_action_admits(current, requested) {
            return Ok(());
        }
        let actor = self
            .substrate
            .entities
            .get_mut(id)
            .ok_or("retired Do_Action receiver")?;
        actor
            .mission_leaf
            .set_infantry_doing_verified(requested)
            .map_err(|error| format!("failed-path Do_Action wrote an invalid Doing: {error:?}"))?;
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

    #[test]
    fn do_action_refuses_unchanged_and_uninterruptible_actions() {
        // Oracle astar_null rows: Doing 0 with request 0 leaves frame/timer alone.
        assert!(!failed_path_do_action_admits(0, 0));
        assert!(failed_path_do_action_admits(-1, 0));
        // Walk (3) is interruptible in the 0x7EAF7C table; Die1 (11) is not.
        assert!(failed_path_do_action_admits(3, 0));
        assert!(!failed_path_do_action_admits(11, 0));
    }
}
