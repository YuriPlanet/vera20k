//! Walk75AEC0's no-queue path request and its Infantry receivers.
//!
//! The caller arms its timer (0x75AF69..0x75AF8F) and enters the shared
//! `FootClass::Find_Path` owner (`foot_path.rs`). A failed search reaches the
//! Infantry receiver `+0x500` (0x0051DAF0) inside Find_Path, then the Walk
//! continuation 0x75AFD3 here. Original caller comparisons:
//! tools/spatial_oracle/walk_failed_path.

use super::foot_path::{FindPathResult, coord_cell};
use super::ground_pose;
use super::infantry_entry::InfantryEntryArgs;
use super::movement_tick::FootPathRequest;
use crate::map::resolved_terrain::NativeCellQuery;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::components::DriveCoord;
use crate::sim::world::FrameEffects;
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
    /// Walk Stop75ADA0: clear destination, and only with no paid head clear
    /// both motion bytes then invoke the owner +54C before returning.
    /// Original Stop/callback controls: infantry_deploy_action.json. This
    /// dispatch also preserves Do_Action's zero-health synchronous re-entry.
    pub(crate) fn walk_stop_moving(
        &mut self,
        id: u64,
        rules: Option<&RuleSet>,
        frame_effects: FrameEffects<'_>,
    ) -> Result<(), String> {
        let actor = self
            .substrate
            .entities
            .get_mut(id)
            .ok_or("Walk Stop receiver retired")?;
        let invoke_callback = actor
            .locomotor
            .as_mut()
            .ok_or("Walk Stop requires a locomotor")?
            .stop_walk();
        if invoke_callback {
            self.infantry_pending_deploy_stop_callback(id, rules, frame_effects)?;
        }
        Ok(())
    }

    /// Synchronous75AFC5 -> Foot4D3920. A successful result resumes this same
    /// Process invocation; a failed result owns its cleanup and must never
    /// enter the arrival finalizer (which would snap the actor's exact XYZ).
    pub(crate) fn run_walk_path_request(
        &mut self,
        request: &FootPathRequest,
        rules: Option<&RuleSet>,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> Result<bool, String> {
        let rules = rules.ok_or("Walk path request requires rules")?;
        let id = request.entity_id;
        let frame = self.session.binary_frame();
        //75AF69..8F arms the caller timer before Foot4D3920.
        self.substrate
            .entities
            .get_mut(id)
            .ok_or("retired Walk path requester")?
            .navigation
            .path_runtime_mut()
            .start_movement(frame, rules.general.path_delay_ticks());
        match self.foot_find_path(request, rules, registry, frame_effects)? {
            FindPathResult::Route => {
                //75B2DF..E2 is the success caller's retry reset. Existing
                //head production resumes at its ordinary shared owner.
                self.substrate
                    .entities
                    .get_mut(id)
                    .ok_or("retired Walk path requester")?
                    .navigation
                    .path_runtime_mut()
                    .reload_retries();
                self.walk_short_path_receiver(id, rules)?;
                Ok(true)
            }
            FindPathResult::Failed => {
                //75AFD5 clears +36 before anything else. The head is null on
                //this path (75AECD), so every failed exit's Stop clears it
                //again and nothing reads it in between. 75AFD3: after a
                //precheck refusal (no receiver) or a core failure (the
                //Infantry receiver already Stopped Walk), the locomotor's own
                //zone recheck reads the retained destination.
                if let Some(locomotor) = self
                    .substrate
                    .entities
                    .get_mut(id)
                    .and_then(|actor| actor.locomotor.as_mut())
                {
                    locomotor.stop_movement_animation();
                }
                self.finish_failed_walk_process(id, rules, registry, frame_effects)?;
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
    /// locomotor +0x48 ([`Self::locomotor_stop_moving`]): Walk's 0x75ADA0
    /// clears the destination and, with no paid head, the IsMoving byte; a
    /// Jumpjet's re-targets the nearest passable cell (a Scenario draw for
    /// Infantry placement); a Teleport's drops an armed warp.
    pub(crate) fn infantry_stop_driver(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
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
            type_id: &object.id,
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
            .is_some_and(|infantry| infantry.is_prone());
        let on_bridge = actor.on_bridge();
        let coord = ground_pose::position_world_coord(&actor.position);
        //0x51DB68..0x51DB7A: the 16-bit facing (+388) becomes an octant through
        //`((facing >> 12) + 1) >> 1 & 7`.
        let direction = i32::from(crate::util::direction_tables::dir_from_facing16(
            actor.body_facing_current(self.session.binary_frame()),
        ));
        let requested = failed_path_requested_action(doing, prone);
        self.apply_infantry_do_action(id, requested, false, &facts, rules, frame_effects)?;

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
                    .record_cell_entry(answer.is_nonzero());
            }
            Err(cause) => log::debug!("infantry {id} Stop_Driver Can_Enter_Cell: {cause}"),
        }
        if actor.locomotor.is_none() {
            return Err("Stop_Driver requires a locomotor".into());
        }
        //0x4D55C0 -> ILocomotion +0x48.
        self.locomotor_stop_moving(id, Some(rules), registry, frame_effects)
    }

    fn finish_failed_walk_process(
        &mut self,
        id: u64,
        rules: &RuleSet,
        _registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
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
            self.assign_null_destination(id, Some(rules), None, frame_effects);
        } else {
            //75AFEE..75B083: +4F4 precedes a fresh read of physical+48 and
            //the live destination. CloseEnough is independent from code6's
            //radio/height/land guards; this arm tests Techno+418 tether only.
            self.walk_failed_path_receiver(id, rules, frame_effects)?;
            let actor = self
                .substrate
                .entities
                .get(id)
                .ok_or("retired failed Walk actor")?;
            let current = ground_pose::position_world_coord(&actor.position);
            let destination = actor
                .locomotor
                .as_ref()
                .and_then(|loco| loco.walk_destination())
                .unwrap_or(DriveCoord { x: 0, y: 0, z: 0 });
            let close = crate::sim::cell_kernel::native_xyz_distance(
                current.x.wrapping_sub(destination.x),
                current.y.wrapping_sub(destination.y),
                current.z.wrapping_sub(destination.z),
            ) < rules.general.close_enough;
            if close && actor.tether.partner().is_none() {
                self.assign_null_destination(id, Some(rules), None, frame_effects);
            } else if actor.navigation.path_runtime().retries_left() != 0 {
                //The test is before the decrement: 1->0 does not run the
                //exhaustion tail until a later failed Process invocation.
                self.substrate
                    .entities
                    .get_mut(id)
                    .unwrap()
                    .navigation
                    .path_runtime_mut()
                    .decrement_retries_left();
            } else {
                self.finish_exhausted_walk_retry(id, rules, frame_effects)?;
            }
        }
        let actor = self
            .substrate
            .entities
            .get_mut(id)
            .ok_or("retired failed Walk actor")?;
        //75B2BC..75B2DC: all failed-queue exits set speed0 then Stop;
        //Stop does not itself clear NavCom or retire the paid head.
        actor
            .foot_speed
            .set_speed_fraction(crate::util::fixed_math::SIM_ZERO);
        self.walk_stop_moving(id, Some(rules), frame_effects)
    }

    ///75B085..75B2BA, reached only with a preexisting zero retry count.
    ///The two Map56D100 calls share the Foot navigation/bridge-layer query
    ///owner but deliberately bypass+2CC's MZ/Cell0 early exits. No RNG draw.
    ///The optional ScoldSound and unconditional latch clear precede these reads.
    fn finish_exhausted_walk_retry(
        &mut self,
        id: u64,
        rules: &RuleSet,
        frame_effects: FrameEffects<'_>,
    ) -> Result<(), String> {
        Self::play_foot_path_scold(&self.substrate, &mut self.sound_events, id, rules);
        self.substrate
            .entities
            .get_mut(id)
            .ok_or("retired exhausted Walk actor")?
            .navigation
            .path_runtime_mut()
            .clear_scold_latch(); //75B0A9, even when the guard was zero.
        let actor = self
            .substrate
            .entities
            .get(id)
            .ok_or("retired exhausted Walk actor")?;
        let destination = actor
            .locomotor
            .as_ref()
            .and_then(|loco| loco.walk_destination())
            .unwrap_or(DriveCoord { x: 0, y: 0, z: 0 });
        if actor.in_playfield() && !self.foot_can_reach_navigation_cell(id, destination, rules)? {
            self.assign_null_destination(id, Some(rules), None, frame_effects);
        }
        //75B18A asks a nonnull target's+4C even when3D5 will then be false.
        let target = self
            .substrate
            .entities
            .get(id)
            .and_then(|e| e.attack_target.target());
        if let Some(target) = target {
            let coordinate = super::navcom::nav_target_coordinate(
                target.into(),
                Some(id),
                self,
                self.resolved_terrain.as_ref(),
                Some((rules, &self.interner)),
            )?;
            let actor = self
                .substrate
                .entities
                .get(id)
                .ok_or("retired exhausted Walk actor")?;
            if actor.in_playfield()
                && actor.attack_target.target().is_some()
                && !self.foot_can_reach_navigation_cell(id, coordinate, rules)?
            {
                self.assign_target_represented(id, None, Some(rules), None, frame_effects)
                    .map_err(|error| format!("{error:?}"))?;
            }
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
