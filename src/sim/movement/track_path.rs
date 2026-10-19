//! Drive/Ship `Process_Movement`'s no-queue path request and its two
//! continuations, plus the Unit class setters they reach.
//!
//! Drive 0x4B2630 and Ship 0x6A1C80 are instruction twins in these arms
//! (aligned in the track-process-path handoff R1); each Drive address below
//! is followed by its Ship twin. Order within one Process call:
//! - 0x4B281C..0x4B2845 / 0x6A1E6C..0x6A1E95: the Foot+640 exact-zero wait
//!   (`track_fresh::track_no_queue_arm`);
//! - 0x4B284B..0x4B286D / 0x6A1EA0..0x6A1EBD: +640 = (Frame, PathDelay);
//! - 0x4B28A3 / 0x6A1EF3: `Find_Path(cell(dest), 0, 0)` (`foot_path.rs`);
//! - success 0x4B2F45..0x4B32A1 / 0x6A2595..0x6A28F1: the tube return, the
//!   next-cell +1AC answer (code 3 gate request, code 6 ally stop or
//!   Scatter_Objects), +64C = 10, then head selection in the SAME call;
//! - failure 0x4B28B3..0x4B2F42 / 0x6A1F03..0x6A2592: the zone recheck, the
//!   CloseEnough stop, the front-cell answer, the +64C retry ladder and the
//!   0x4B2E77 tail.
//!
//! The outer Process calls Process_Track after every return (0x4B0AAA /
//! 0x6A0173) unless the Foot vanished (out byte, 0x4B28BE / 0x6A1F0E). When
//! Process_Track(0) ends a track, the same Process reaches this owner again
//! (`track_continuation`).
//!
//! Reach of the failure ladder: Find_Path's Unit receiver (+0x500 =
//! 0x4D55C0 -> locomotor Stop) clears the destination on every core failure,
//! and a precheck refusal repeats in the recheck, so the recheck
//! (0x4B28CD) returns through SetDestination(NULL) unless the continuation
//! 0x4D41C2 relocated a nonhuman Foot through House500200 in a nonzero game
//! mode (`foot_path::finish_find_path_failure`). The ladder runs after that
//! relocation.
//!
//! Residuals:
//! - 24-word copy. Find_Path copies at most 24 - prefix words into Foot+5E0
//!   (0x4D3E82..0x4D3E9F); VERA installs the whole route, so the native
//!   re-request every 24 cells does not happen. Effect: a long route is not
//!   re-planned mid-way.

use super::foot_path::FootPathOutcome;
use super::ground_pose;
use super::infantry_entry::InfantryEntryArgs;
use super::movement_tick::FootPathRequest;
use crate::map::entities::EntityCategory;
use crate::map::resolved_terrain::NativeCellQuery;
use crate::rules::locomotor_type::LocomotorKind;
use crate::rules::mission_data::MissionType;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::components::{DriveCoord, NavTargetRef};
use crate::sim::game_entity::GameEntity;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::world::FrameEffects;
use crate::sim::world::Simulation;
use crate::util::direction::DIRECTION_DELTAS;
use crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS;
use crate::util::native_x87::distance_3d_leptons;

/// A Unit on Drive or Ship, whose class setter is Unit 0x741970.
fn track_unit(entity: &GameEntity) -> bool {
    entity.category() == EntityCategory::Unit
        && entity.locomotor.as_ref().is_some_and(|loco| {
            matches!(
                loco.active_kind(),
                LocomotorKind::Drive | LocomotorKind::Ship
            )
        })
}

/// A Unit on a Hover locomotor: the Unit setter's Foot tail reaches Hover
/// Move_To (0x00514D90) and Stop_Moving (0x00516320).
fn hover_unit(entity: &GameEntity) -> bool {
    entity.category() == EntityCategory::Unit
        && entity
            .locomotor
            .as_ref()
            .is_some_and(|loco| loco.active_kind() == LocomotorKind::Hover)
}

/// A Unit on a Walk locomotor (no retail VehicleType; a map may assign one):
/// the Unit setter's Foot tail reaches Walk Move_To (0x0075ACB0).
fn walk_unit(entity: &GameEntity) -> bool {
    entity.category() == EntityCategory::Unit
        && entity
            .locomotor
            .as_ref()
            .is_some_and(|loco| loco.active_kind() == LocomotorKind::Walk)
}

/// A Unit on a Teleport locomotor, which for a type that is no `Teleporter=`
/// is the Chrono Warp's (`SuperClass::Launch @ 0x006CCB4A`): the Unit
/// setter's Foot tail reaches Teleport Move_To (`0x00718100`).
fn teleport_unit(entity: &GameEntity) -> bool {
    entity.category() == EntityCategory::Unit
        && entity
            .locomotor
            .as_ref()
            .is_some_and(|loco| loco.active_kind() == LocomotorKind::Teleport)
}

/// A Unit on a Jumpjet locomotor: its null destination reaches the Jumpjet's
/// `Stop_Moving` through the same Unit setter. The Foot tail then calls
/// `JumpjetLocomotionClass::Move_To` (`0x0054B1C0`) or `Stop_Moving` (`0x0054B4D0`).
fn jumpjet_unit(entity: &GameEntity) -> bool {
    entity.category() == EntityCategory::Unit
        && entity
            .locomotor
            .as_ref()
            .and_then(|loco| loco.jumpjet_runtime())
            .is_some()
}

/// A `Teleporter=` Unit (TechnoType+0xCD4): Unit 0x741970 runs its
/// Teleporter arm, which swaps a Drive in over the Teleport primary or ends
/// it, before the Foot tail.
fn teleporter_unit(sim: &Simulation, entity: &GameEntity, rules: Option<&RuleSet>) -> bool {
    entity.category() == EntityCategory::Unit
        && rules
            .and_then(|rules| sim.object_type(entity.type_ref(), rules))
            .is_some_and(|object| object.teleporter)
}

/// `0x00741A96..0x00741ACD`: Unit Set_Destination refuses while the Unit
/// deploys either way (`+0x6E1`, `+0x6E2`), and while it is deployed
/// (`+0x6E0`) unless a Magnetron holds it (`+0x2B0`, `0x00741AC3`).
fn deployment_refuses_destination(actor: &GameEntity) -> bool {
    match actor.mission_leaf.as_unit() {
        Some(leaf) if leaf.deployed() != 0 => actor.lift.source().is_none(),
        _ => actor.is_deployed(),
    }
}

/// The active Drive/Ship locomotor's Destination (`+0x34`).
pub(super) fn track_destination(entity: &GameEntity) -> Option<DriveCoord> {
    let loco = entity.locomotor.as_ref()?;
    loco.track_destination(super::track_process::TrackFamily::from_kind(
        loco.active_kind(),
    )?)
}

/// [`track_destination`] as the reads without a null test take it: the
/// null coordinate (0,0,0) when there is none, which Stop_Moving writes
/// (Drive `0x004AFEA7..0x004AFEC0`).
pub(super) fn track_destination_or_null(entity: &GameEntity) -> DriveCoord {
    track_destination(entity).unwrap_or(super::foot_coordinate::NULL_COORD)
}

/// `if (head != Null) { head = Null; +63 = 0; }` as every arm here writes it.
/// Raw occupation marks are untouched, as in native.
pub(super) fn clear_track_head(entity: &mut GameEntity) {
    if let Some(loco) = entity.locomotor.as_mut()
        && let Some(family) = super::track_process::TrackFamily::from_kind(loco.active_kind())
        && loco.track_head(family).is_some()
    {
        loco.store_track_head(family, None);
        loco.store_track_valid(family, false);
    }
}

/// `((Current >> 12) + 1) >> 1 & 7` over the body FacingClass (0x4B2A88..94).
fn facing_octant(entity: &GameEntity, frame: u32) -> u8 {
    crate::util::direction_tables::dir_from_facing16(entity.body_facing_current(frame))
}

/// Techno+0x1F8, constructor zero (`0x006F2CE1`). The Unit setter's
/// Teleporter arm raises it when a Drive cannot end yet (`0x007425C6`), so
/// the next setter call runs for an unchanged NavCom (`0x00741A88`); every
/// setter call that passes that check clears it (`0x00741A9C`). Unlimbo's
/// own raise and clear (`0x006F6E1B`/`0x006F6E34`) bracket one call and
/// leave nothing behind. Only this module's setters write it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub(crate) struct SetterForceReassign {
    raised: bool,
}

impl SetterForceReassign {
    /// The next setter call runs for an unchanged NavCom.
    pub(crate) const fn is_raised(self) -> bool {
        self.raised
    }

    /// Install a fixture's latch.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_for_test(&mut self, raised: bool) {
        self.raised = raised;
    }
}

/// The destination the Unit setter's Dock arm leaves
/// ([`Simulation::unit_dock_arm`]).
enum UnitDockArm {
    Keep,
    Cell((u16, u16)),
    Null,
}

impl Simulation {
    /// Drive4B28A3 / Ship6A1EF3 and the continuation that follows it.
    pub(crate) fn run_track_path_request(
        &mut self,
        request: &FootPathRequest,
        rules: Option<&RuleSet>,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> Result<FootPathOutcome, String> {
        let rules = rules.ok_or("Drive/Ship path request requires rules")?;
        let id = request.entity_id;
        let frame = self.session.binary_frame;
        //4B284B..4B286D / 6A1EA0..6A1EBD arm the caller timer first.
        self.substrate
            .entities
            .get_mut(id)
            .ok_or("retired Drive/Ship path requester")?
            .navigation
            .path_runtime
            .start_movement(frame, rules.general.path_delay_ticks());
        let found = self.foot_find_path(request, rules, registry, frame_effects)?;
        self.continue_track_path_request(id, found, rules, registry, frame_effects)
    }

    /// The continuation after `Find_Path` returned (0x4B28A8 / 0x6A1EF8);
    /// tools/spatial_oracle/track_path_continuation supplies its result.
    pub(crate) fn continue_track_path_request(
        &mut self,
        id: u64,
        found: super::foot_path::FindPathResult,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> Result<FootPathOutcome, String> {
        use super::foot_path::FindPathResult;
        //4B28B3..4B28C7 / 6A1F03..6A1F17: a vanished Foot sets the out byte.
        if self.substrate.entities.get(id).is_none() {
            return Ok(FootPathOutcome::Deleted);
        }
        match found {
            FindPathResult::Route => {
                self.finish_found_track_path(id, rules, registry, frame_effects)
            }
            FindPathResult::Failed => {
                self.finish_failed_track_path(id, rules, registry, frame_effects)
            }
        }
    }

    /// 0x4B2F45..0x4B32A1 / 0x6A2595..0x6A28F1 after a found route.
    fn finish_found_track_path(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> Result<FootPathOutcome, String> {
        let actor = self
            .substrate
            .entities
            .get(id)
            .ok_or("retired Drive/Ship path owner")?;
        let &direction = actor
            .navigation
            .path_replay
            .remaining_directions()
            .first()
            .ok_or("Find_Path route left no Foot+5E0 head")?;
        //4B2F45 / 6A2595: a tube step returns before any cell answer.
        if direction == crate::util::direction::TUBE_STEP_DIRECTION {
            return Ok(FootPathOutcome::Returned);
        }
        let (dx, dy) = DIRECTION_DELTAS[usize::from(direction & 7)];
        //4B2F52..4B2FB2 steps from the Location's cell (vt+48 = 5F65A0).
        let location = ground_pose::position_world_coord(&actor.position);
        let current = super::foot_path::coord_cell(location);
        let next = (i32::from(current.0) + dx, i32::from(current.1) + dy);
        let stop =
            self.answer_track_ahead_cell(id, next, direction, rules, registry, frame_effects)?;
        if stop {
            return Ok(FootPathOutcome::Returned);
        }
        //4B3282..4B328F / 6A28D2..6A28DF: the found-route retry reload.
        self.substrate
            .entities
            .get_mut(id)
            .ok_or("retired Drive/Ship path owner")?
            .navigation
            .path_runtime
            .set_retries_left(crate::sim::components::FootPathRuntime::RETRY_RELOAD);
        Ok(FootPathOutcome::Resume)
    }

    /// 0x4B28CA..0x4B2F42 / 0x6A1F1A..0x6A2592 after `Find_Path` refused.
    fn finish_failed_track_path(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> Result<FootPathOutcome, String> {
        let destination = self.substrate.entities.get(id).and_then(track_destination);
        //4B28CD / 6A1F1D: the zone precheck on the LIVE destination, which the
        //Unit receiver may already have cleared (Cell(0,0) refuses).
        let probe = destination.unwrap_or(super::foot_coordinate::NULL_COORD);
        if !self.foot_path_zone_precheck(id, probe, rules)? {
            self.set_unit_null_destination(id, Some(rules), None, frame_effects);
            return Ok(FootPathOutcome::Returned);
        }
        //4B28F5..4B2917: a null destination returns.
        let Some(destination) = destination else {
            return Ok(FootPathOutcome::Returned);
        };
        let actor = self
            .substrate
            .entities
            .get(id)
            .ok_or("retired Drive/Ship path owner")?;
        let mission = actor.mission.effective().known();
        let location = ground_pose::position_world_coord(&actor.position);
        //4B291D..4B29A9: not Enter, within CloseEnough of the destination
        //(Distance3D, 4B2974), and a Move or AreaGuard mission.
        if mission != Some(MissionType::Enter)
            && distance_3d_leptons(
                [location.x, location.y, location.z],
                [destination.x, destination.y, destination.z],
            ) < rules.general.close_enough
            && matches!(mission, Some(MissionType::Move | MissionType::AreaGuard))
        {
            //4B29AF..4B2A44: clear the head, then stop or take the waypoint.
            if self.stop_or_take_next_waypoint(id, rules, frame_effects) {
                return Ok(FootPathOutcome::Returned);
            }
            //4B2A06..4B2A11: a dead Foot returns before the tail.
            if !self.track_owner_alive(id) {
                return Ok(FootPathOutcome::Returned);
            }
            return self.finish_track_path_tail(id, rules, registry, frame_effects);
        }
        //4B2A47..4B2B17 / 6A2097..: the cell ahead of the body facing.
        let frame = self.session.binary_frame;
        let octant = facing_octant(actor, frame);
        let current = super::foot_path::coord_cell(location);
        let (dx, dy) = DIRECTION_DELTAS[usize::from(octant)];
        let ahead = (i32::from(current.0) + dx, i32::from(current.1) + dy);
        if self.answer_track_ahead_cell(id, ahead, octant, rules, registry, frame_effects)? {
            return Ok(FootPathOutcome::Returned);
        }
        //4B2DC5..4B2DD9 / 6A2415..: spend one retry, else give up.
        let actor = self
            .substrate
            .entities
            .get_mut(id)
            .ok_or("retired Drive/Ship path owner")?;
        if actor.navigation.path_runtime.retries_left() > 0 {
            actor.navigation.path_runtime.decrement_retries_left();
            return self.finish_track_path_tail(id, rules, registry, frame_effects);
        }
        //4B2DDE..4B2E30: the exhausted ladder.
        if self.stop_or_take_next_waypoint(id, rules, frame_effects) {
            return Ok(FootPathOutcome::Returned);
        }
        //4B2E36..4B2E70 / 6A2486..6A24C0: a dead Foot returns;
        //otherwise request the retained +68A sound, then clear the byte.
        if !self.track_owner_alive(id) {
            return Ok(FootPathOutcome::Returned);
        }
        self.play_foot_path_scold(id, rules);
        self.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .navigation
            .path_runtime
            .clear_scold_latch();
        self.finish_track_path_tail(id, rules, registry, frame_effects)
    }

    /// Whether the Foot can fire at its TarCom (vt+0x3AC, TechnoClass::
    /// CanFireAtTarget 0x6F7780); None without one.
    pub(super) fn foot_can_fire_at_target(
        &self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) -> Option<bool> {
        let target = self.substrate.entities.get(id)?.attack_target.target()?;
        Some(self.techno_can_fire_at_target(id, target, rules, registry))
    }

    /// A stopped Foot's failed-route tail (Drive 0x4B2E92, Ship 0x6A24C7,
    /// Hover 0x00516822): a TarCom it cannot fire at sets +688 before a team
    /// member's team drops its targets (TeamClass::Scan_Limit 0x6EC3A0, which
    /// invokes each member's class target setter and sets its latch too),
    /// then TarCom is cleared.
    pub(super) fn drop_unfireable_target(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) {
        if self.foot_can_fire_at_target(id, rules, registry) != Some(false) {
            return;
        }
        if let Some(actor) = self.substrate.entities.get_mut(id) {
            actor.foot_retarget_after_stop.mark_stopped_cannot_fire();
        }
        if let Some((team_id, _)) = self.team_script_vm.team_for_member(id) {
            self.team_scan_limit(team_id, rules, registry, frame_effects);
        }
        let _ = self.assign_target_represented(id, None, Some(rules), registry, frame_effects);
    }

    /// 0x4B2E77..0x4B2F1A / 0x6A24C7..: a stopped Foot holding a target it
    /// cannot fire at drops it; then the head and selector are cleared.
    fn finish_track_path_tail(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> Result<FootPathOutcome, String> {
        let actor = self
            .substrate
            .entities
            .get(id)
            .ok_or("retired Drive/Ship path owner")?;
        if !super::motion_query::is_moving(actor).unwrap_or(false) {
            self.drop_unfireable_target(id, rules, registry, frame_effects);
        }
        let actor = self
            .substrate
            .entities
            .get_mut(id)
            .ok_or("retired Drive/Ship path owner")?;
        clear_track_head(actor);
        //4B2F07: selector (+58) = -1. +61 has no reader in the program.
        if let Some(loco) = actor.locomotor.as_mut()
            && let Some(family) = super::track_process::TrackFamily::from_kind(loco.active_kind())
            && let Some(mut progress) = loco.track_progress(family)
        {
            progress.retire_selector();
            loco.store_track_progress(family, progress);
        }
        Ok(FootPathOutcome::Returned)
    }

    /// The shared cell answer of both continuations: the height-aware
    /// playfield test 0x578460, then the Unit +1AC with the Techno height
    /// 0x5F5F00; code 3 asks a gate to open (0x578AD0), code 6 takes the
    /// ally arm. Returns true when the ally arm stopped the Foot (the caller
    /// then returns without its retry/tail work).
    fn answer_track_ahead_cell(
        &mut self,
        id: u64,
        cell: (i32, i32),
        direction: u8,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> Result<bool, String> {
        if !crate::sim::cell_rect::cell_is_in_playfield_height_aware(
            cell,
            self.playfield_bounds,
            self.resolved_terrain.as_ref(),
        ) {
            return Ok(false);
        }
        let terrain = self
            .resolved_terrain
            .as_ref()
            .ok_or("Drive/Ship cell answer requires map cells")?;
        let actor = self
            .substrate
            .entities
            .get(id)
            .ok_or("retired Drive/Ship path owner")?;
        let location = ground_pose::position_world_coord(&actor.position);
        let cells = NativeCellQuery::canonical(terrain);
        let height = ground_pose::query_object_cell_height(&cells, location, actor.on_bridge());
        let native_cell = terrain.native_cell_identity((cell.0 as i16, cell.1 as i16));
        let code = self.foot_can_enter(
            id,
            native_cell,
            InfantryEntryArgs {
                direction: i32::from(direction),
                height,
                previous_cell: None,
            },
            rules,
            registry,
        )?;
        match code {
            3 => {
                //4B2B38 / 4B301A: the result of 0x578AD0 is discarded.
                let _ = crate::sim::gate_runtime::request_gate_open_for_cell(
                    self,
                    (cell.0 as u16, cell.1 as u16),
                    id,
                    rules,
                );
                Ok(false)
            }
            6 => self.answer_track_ally_cell(id, cell, rules, registry, frame_effects),
            _ => Ok(false),
        }
    }

    /// The code-6 arm (0x4B2B4B..0x4B2DC0 / 0x4B302D..0x4B327D and the Ship
    /// twins): the nearest Techno of the selected list (0x47C3D0 with the
    /// (0,0) sub-point) that is an ally of a non-train owner either ends the
    /// move — close enough to the destination, no radio contact, the same
    /// height band, not standing in a Tunnel — or has the cell scattered.
    fn answer_track_ally_cell(
        &mut self,
        id: u64,
        cell: (i32, i32),
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> Result<bool, String> {
        let terrain = self
            .resolved_terrain
            .as_ref()
            .ok_or("Drive/Ship ally arm requires map cells")?;
        let cells = NativeCellQuery::canonical(terrain);
        let actor = self
            .substrate
            .entities
            .get(id)
            .ok_or("retired Drive/Ship path owner")?;
        let location = ground_pose::position_world_coord(&actor.position);
        let centre = DriveCoord {
            x: cell.0 * 256 + 128,
            y: cell.1 * 256 + 128,
            z: 0,
        };
        //4B2BB2..4B2BC6: the deck list when the Foot rides above the ground
        //at the cell centre by more than two levels.
        let ground = ground_pose::query_ground_height(&cells, centre)?;
        let alt = location.z > ground + 2 * GROUND_LEVEL_HEIGHT_LEPTONS;
        let layer = if alt {
            MovementLayer::Bridge
        } else {
            MovementLayer::Ground
        };
        let key = (cell.0 as u16, cell.1 as u16);
        let Some(blocker) = self.nearest_cell_object(key, layer, None) else {
            return Ok(false);
        };
        //4B2BDA..4B2BE4: the Foot's own House asks (Is_Ally_ByObject).
        let allied = self.substrate.entities.get(blocker).is_some_and(|b| {
            crate::sim::combat::combat_weapon::is_ally_by_object(
                Some(&self.house_alliances),
                &self.interner,
                actor.owner(),
                b.owner(),
            )
        });
        //4B2BF1..4B2C04: IsTrain (Type+C94) is set by no retail TechnoType.
        if !allied {
            return Ok(false);
        }
        let destination = track_destination_or_null(actor);
        //4B2C14..4B2C62: Distance3D (0x41C380) below CloseEnough, then no
        //radio contact (0x65AE30) and the shared stop band.
        let close = distance_3d_leptons(
            [location.x, location.y, location.z],
            [destination.x, destination.y, destination.z],
        ) < rules.general.close_enough;
        if close && actor.radio_contacts.is_empty() && self.track_stop_band(location, destination) {
            //4B2CDD..4B2D65: clear the head, then stop or take the waypoint.
            self.stop_or_take_next_waypoint(id, rules, frame_effects);
            return Ok(true);
        }
        //4B2D68..4B2DC0: the forced scatter of the refused cell.
        self.scatter_blocked_track_cell(
            id,
            (cell.0 as i16, cell.1 as i16),
            rules,
            registry,
            frame_effects,
        )?;
        Ok(false)
    }

    /// The shared stop/waypoint pair: clear the head; with an empty NavQueue
    /// SetDestination(NULL, 1), else Foot 0x4DF0D0 (NavCom only) then the
    /// Unit idle entry (+0x484 = 0x738970), whose AL the caller may return.
    /// Returns that AL (false after the NULL setter).
    pub(super) fn stop_or_take_next_waypoint(
        &mut self,
        id: u64,
        rules: &RuleSet,
        frame_effects: FrameEffects<'_>,
    ) -> bool {
        let Some(actor) = self.substrate.entities.get_mut(id) else {
            return false;
        };
        clear_track_head(actor);
        if actor.navigation.nav_queue().is_empty() {
            self.set_unit_null_destination(id, Some(rules), None, frame_effects);
            return false;
        }
        actor.navigation.stop_moving();
        self.unit_enter_idle_mode(id, Some(rules), false, frame_effects)
    }

    /// The outer Process stop at a same-cell Cell NavCom (0x4B066C..0x4B06D2)
    /// or a Guard exact destination (0x4B06D5..0x4B0772), Ship twins
    /// 0x69FD13 / 0x69FD7F: NavQueue empty -> SetDestination(NULL, 1), else
    /// Foot 0x4DF0D0 then the Unit idle entry (+0x484). No head write.
    pub(super) fn track_navcom_stop(
        &mut self,
        id: u64,
        rules: Option<&RuleSet>,
        frame_effects: FrameEffects<'_>,
    ) {
        let Some(actor) = self.substrate.entities.get_mut(id) else {
            return;
        };
        if actor.navigation.nav_queue().is_empty() {
            self.set_unit_null_destination(id, rules, None, frame_effects);
        } else {
            actor.navigation.stop_moving();
            self.unit_enter_idle_mode(id, rules, false, frame_effects);
        }
    }

    /// The outer Process zone drop (0x4B09EC..0x4B0A68 / 0x6A00B5..0x6A0131):
    /// clear the head, then the shared stop/waypoint pair.
    pub(super) fn track_zone_drop(
        &mut self,
        id: u64,
        rules: &RuleSet,
        frame_effects: FrameEffects<'_>,
    ) {
        self.stop_or_take_next_waypoint(id, rules, frame_effects);
    }

    /// Foot+90 as Drive re-reads it after a synchronous setter.
    pub(super) fn track_owner_alive(&self, id: u64) -> bool {
        self.substrate
            .entities
            .get(id)
            .is_some_and(|e| e.lifecycle.object_alive())
    }

    /// The one completion of a class-setter call Rust deferred (the
    /// `pending_destination_setter` flag): a mission restore's
    /// `Assign_Destination(saved, 1)` (`FootClass::Restore_Mission`, call at
    /// `0x004D8F99`), which `mission::authority` represents by NavCom alone
    /// where the setter cannot run. It runs at the object's next Process entry
    /// or, after a track end, before the same-call continuation's gates
    /// (`track_continuation`), where natively the setter already ran.
    ///
    /// It makes the call through the class setters
    /// ([`Self::assign_destination_represented`]) when one takes it
    /// ([`Self::destination_setter_available`]); otherwise the receiver keeps
    /// the represented NavCom. NavCom is cleared first, so the Unit setter's
    /// unchanged-NavCom return cannot swallow the call: the NavCom the native
    /// call sees is the override's (Override_Mission saved NavCom at
    /// `0x004D8F5B` and set its own at `0x004D8F6D`), not the saved one.
    ///
    /// Residual: native Restore runs this setter inside the restore; VERA runs
    /// it at the object's next Process entry, without the override's NavCom.
    /// Trigger: an AoE cell target expiring under a suspended mission
    /// (`restore_entity_after_target_expiry`, whose broadcast holds only the
    /// entity store), or a Restore made without rules. Effects:
    /// - the setter's timers, NavQueue clear, Move_To and admission effects
    ///   (the deployment refusal's NavCom clear, the UnitRepair radio arm, the
    ///   Teleporter arm's locomotor swap) land up to one frame late, inside the
    ///   Process host chosen before the call;
    /// - an override NavCom equal to the saved one takes the whole setter,
    ///   where native's unchanged-NavCom return (`0x00741A80`) writes nothing;
    /// - a setter that refuses before the Foot tail leaves NavCom and
    ///   NavComAux empty, where native keeps the override's.
    ///
    /// Frequency: rare. Risk: timing, plus the rare same-NavCom and refusal
    /// cases. The Teleporter swap needs a `Teleporter=` Unit holding a cell
    /// target; a text search of retail `rulesmd.ini` finds only the unarmed
    /// CMIN among Units.
    pub(crate) fn complete_pending_order(
        &mut self,
        id: u64,
        rules: Option<&RuleSet>,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) {
        let Some(rules) = rules else {
            return;
        };
        let Some(actor) = self.substrate.entities.get_mut(id) else {
            return;
        };
        if !actor.navigation.take_destination_setter() {
            return;
        }
        // A NavCom cleared after the restore (a Stop) leaves nothing owed.
        let Some(requested) = actor.navigation.nav_com() else {
            return;
        };
        if !self.destination_setter_available(id, requested, rules, registry) {
            return;
        }
        if let Some(actor) = self.substrate.entities.get_mut(id) {
            actor.navigation.stop_moving();
        }
        self.assign_destination_represented(
            id,
            Some(requested),
            Some(rules),
            registry,
            frame_effects,
        )
        .expect("checked deferred destination setter");
    }

    /// The class setter vt+0x480 with a NULL destination from outside the
    /// locomotor: Event STOP (0x4C75ED) and MEGAMISSION (0x4C747C, an
    /// ordered Attack's), Foot::Override_Mission (0x4D8F6D, the
    /// ReceiveDamage retaliation) and Restore_Mission (0x4D8F99), the Stuns
    /// (Foot 0x4D5669, Techno 0x6FCD55), the capture reset (0x70F859), the
    /// owner change (0x7014E9, 0x70182F), the parasite release (0x62A78A,
    /// 0x62A3ED, 0x62AAB9) and the Temporal freeze. Each class's setter
    /// (vtables 0x7F5C70, 0x7EB058 and 0x7E22A4, whatever the locomotor) ends
    /// in Foot's null arm ([`Self::foot_null_destination`]) and so in its
    /// active locomotor's Stop_Moving:
    /// - a Unit takes Unit 0x741970 ([`Self::set_unit_null_destination`]);
    /// - an Infantry takes Infantry 0x51AA40
    ///   ([`Self::set_infantry_null_destination`]), which may refuse a human
    ///   deploy action before any write;
    /// - an Aircraft's null arm (0x41AA8B -> 0x41ADAC) is Foot's own, whose
    ///   Fly Stop re-targets a moving aircraft (`Simulation::fly_stop_moving`).
    ///
    /// A Building takes Building455D50 ([`Self::set_building_destination`]):
    /// it clears an eligible rally ArchiveTarget and never writes Foot NavCom.
    pub(crate) fn assign_null_destination(
        &mut self,
        id: u64,
        rules: Option<&RuleSet>,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) {
        let Some(category) = self
            .substrate
            .entities
            .get(id)
            .map(|actor| actor.category())
        else {
            return;
        };
        match category {
            EntityCategory::Unit => {
                self.set_unit_null_destination(id, rules, registry, frame_effects);
            }
            EntityCategory::Infantry => {
                self.set_infantry_null_destination(id, rules, registry, frame_effects);
            }
            EntityCategory::Aircraft => {
                self.foot_null_destination(id, rules, registry, frame_effects)
            }
            EntityCategory::Structure => self.set_building_destination(id, None, rules),
        }
    }

    /// Whether the Unit setter (`0x741970`) is represented for `id` as the
    /// team, harvest and undeploy callers use it: a Drive, Ship, Hover, Walk,
    /// Jumpjet or Teleport receiver, or a `Teleporter=` Unit whatever its
    /// locomotor.
    pub(crate) fn unit_setter_receiver(&self, id: u64, rules: Option<&RuleSet>) -> bool {
        self.substrate.entities.get(id).is_some_and(|actor| {
            track_unit(actor)
                || hover_unit(actor)
                || walk_unit(actor)
                || jumpjet_unit(actor)
                || teleport_unit(actor)
                || teleporter_unit(self, actor, rules)
        })
    }

    /// Unit 0x741970(target, clear_queue). Calls whose receiver may be of any
    /// class reach it through the class-setter dispatcher
    /// ([`Self::assign_destination_represented`], mode 1, and
    /// [`Self::assign_destination_keeping_queue`], mode 0: the NavQueue arms
    /// of Enter_Idle_Mode 0x004D8395, Foot Approach 0x004D5A6A and
    /// Mission_Enter 0x004D939A). Unit code calls it directly: Mission_Harvest's
    /// staging destination (0x0073EDB5), the Scatter receiver's null arm
    /// (0x00744070) and the ReceiveDamage crush charge (0x00738593), all with
    /// true.
    /// - 0x741A80..0x741A9C: an unchanged NavCom returns before any write
    ///   unless the Techno+0x1F8 override is up; the call then clears it. So
    ///   the refinery's repeated MOVE_HERE leaves a running drive alone
    ///   (tools/spatial_oracle/track_destination.json `same_nav` rows).
    /// - 0x741BC7..0x741C4A: a `Harvester=` Unit whose mission or queue is
    ///   Unload clears its refinery's Special animation
    ///   ([`crate::sim::miner::clear_unload_anim_for_destination`]).
    /// - A `Teleporter=` type runs its arm ([`Self::unit_teleporter_arm`]).
    /// - The Foot tail (0x4D94B0) writes NavCom and calls the active
    ///   locomotor's Move_To — Drive/Ship ([`set_destination_internal_coord`]),
    ///   Teleport ([`teleport_move_to`]), Jumpjet
    ///   ([`Self::jumpjet_move_to`]), Hover ([`hover_move_to`]) or Walk
    ///   ([`set_walk_destination_coord`]) — unless Foot+0x6AC skips it once.
    ///
    /// Returns false for a receiver without a represented Move_To or a
    /// refused destination. Foot4D9628 calls the destination's +4C coordinate
    /// virtual for both Cell and Object targets before Jumpjet54B1C0.
    ///
    /// [`set_destination_internal_coord`]: super::navcom::set_destination_internal_coord
    /// [`teleport_move_to`]: super::teleport_movement::teleport_move_to
    /// [`hover_move_to`]: super::hover::hover_move_to
    /// [`set_walk_destination_coord`]: super::navcom::set_walk_destination_coord
    pub(crate) fn set_unit_destination(
        &mut self,
        id: u64,
        requested: NavTargetRef,
        rules: &RuleSet,
        clear_queue: bool,
        frame_effects: FrameEffects<'_>,
    ) -> bool {
        let Some(actor) = self.substrate.entities.get(id) else {
            return false;
        };
        let teleporter = teleporter_unit(self, actor, Some(rules));
        let hover = hover_unit(actor);
        let jumpjet = jumpjet_unit(actor);
        if !(track_unit(actor)
            || teleporter
            || hover
            || jumpjet
            || walk_unit(actor)
            || teleport_unit(actor))
            || !super::can_accept_destination(actor)
        {
            return false;
        }
        if actor.navigation.nav_com() == Some(requested) && !actor.setter_force_reassign.raised {
            return true;
        }
        let actor = self
            .substrate
            .entities
            .get_mut(id)
            .expect("same Unit setter");
        actor.setter_force_reassign.raised = false;
        //741A96..741ACD, after the same-NavCom guard: deployment refuses
        // through Foot4DF0D0, which clears NavCom, not locomotor movement.
        if deployment_refuses_destination(actor) {
            actor.navigation.stop_moving();
            return false;
        }
        let is_harvester = self
            .substrate
            .entities
            .get(id)
            .and_then(|actor| self.object_type(actor.type_ref(), rules))
            .is_some_and(|object| object.harvester);
        let requested_techno = match requested {
            NavTargetRef::Object { id } => crate::sim::radio::as_techno(self, id),
            NavTargetRef::Cell { .. } => None,
        };
        // 741AD1..741B22: a transport (`Passengers=` above 0) whose first
        // contact is an infantryman, one walking aboard, breaks that contact
        // when sent to another Techno it holds no contact with.
        if let Some(target) = requested_techno
            && let Some(unit) = self.substrate.entities.get(id)
            && let Some(contact) = unit.radio_contacts.slot(0)
            && !unit.radio_contacts.contains(target)
            && self
                .object_type(unit.type_ref(), rules)
                .is_some_and(|object| object.passengers > 0)
            && self
                .substrate
                .entities
                .get(contact)
                .is_some_and(|contact| contact.category() == EntityCategory::Infantry)
        {
            crate::sim::radio::transmit_to_contact(
                self,
                id,
                crate::sim::radio::RadioMessage::Break,
                Some(rules),
                frame_effects,
            );
        }
        // 741C4F..741C78 decides on the state after that break.
        let enter_radio_arm = {
            let actor = self
                .substrate
                .entities
                .get_mut(id)
                .expect("same setter actor");
            let enter_radio_arm = super::movement_commands::enter_without_contact(actor);
            super::movement_commands::clear_destination_path_head(actor);
            enter_radio_arm
        };
        crate::sim::miner::clear_unload_anim_for_destination(self, rules, id);
        // Unit741C4F..741E8E and742C14..742D0B: the UnitRepair
        // Building arm belongs to this same class setter, including callers
        // that already HELLOed in Foot70D7E0. No caller repeats its handshake.
        // The original incoming Building identity survives a modified NULL
        // argument. Native controls: refinery_dock.json; source741970.
        //
        // The Enter arm docks with a `DockUnload=` refinery the same way: a
        // miner's refinery order (`Command::MinerReturn`). A busy `Bunker=`
        // (`0x00741D49..0x00741D68`) belongs to the bunker adapter.
        //
        // RESIDUAL: the arm for a Techno that is neither a Unit nor a
        // Building (`0x00741E35..0x00741E86`: ArchiveTarget when it is busy,
        // else HELLO, DOCKING and a break unless ROGER) is not ported.
        // Trigger: a Unit ordered to Enter an Aircraft or an infantryman; no
        // retail AircraftType or InfantryType sets `Passengers=`, so neither
        // the input nor team action 14 orders one. Effect: no radio contact
        // is made, and the unit drives to the target.
        let requested_object = match requested {
            NavTargetRef::Object { id } => self
                .substrate
                .entities
                .get(id)
                .map(|target| (id, target.category())),
            _ => None,
        };
        let original_building = requested_object
            .filter(|(_, category)| *category == EntityCategory::Structure)
            .map(|(id, _)| id);
        let enter_building = original_building.and_then(|building| {
            let object =
                self.object_type(self.substrate.entities.get(building)?.type_ref(), rules)?;
            (object.unit_repair || object.dock_unload).then_some((
                building,
                object.unit_repair,
                object.dock_unload,
            ))
        });
        let mut destination = Some(requested);
        let mut depot_fast_return = false;
        // Steam gamemd3e81a617,741C4F..741CCC: an Enter Unit target
        // without a radio contact goes into Foot+500, not straight into
        // NavCom. Foot70D7E0 owns HELLO/CanEnter on the next object turn.
        // Reuse that pending-entry owner for vehicles as well as depots.
        let mobile_target = requested_object
            .filter(|(_, category)| *category == EntityCategory::Unit)
            .map(|(id, _)| id);
        // 741CE5..741D0D: a `UnitAbsorb=` building waits there too, whatever
        // its contacts.
        let absorber = original_building.filter(|&building| {
            self.substrate
                .entities
                .get(building)
                .and_then(|building| self.object_type(building.type_ref(), rules))
                .is_some_and(|object| object.unit_absorb)
        });
        if let Some(target) = mobile_target.or(absorber)
            && enter_radio_arm
        {
            if let Some(unit) = self.substrate.entities.get_mut(id) {
                unit.pending_entry.install(target);
                unit.set_archive_target(None);
            }
            destination = None;
        }
        if let Some((building, unit_repair, dock_unload)) = enter_building
            && absorber.is_none()
        {
            if enter_radio_arm {
                let busy = self
                    .substrate
                    .entities
                    .get(building)
                    .is_some_and(|building| !building.radio_contacts.is_empty());
                if busy {
                    // `0x00741D41..0x00741D44`: a busy building that is not a
                    // Bunker= becomes the ArchiveTarget.
                    if let Some(unit) = self.substrate.entities.get_mut(id) {
                        unit.set_archive_target(Some(crate::sim::combat::TargetKind::Entity(
                            building,
                        )));
                    }
                    if unit_repair {
                        let damaged = self
                            .substrate
                            .entities
                            .get(id)
                            .and_then(|unit| {
                                self.object_type(unit.type_ref(), rules)
                                    .map(|object| !unit.health.is_full(object.strength))
                            })
                            .unwrap_or(false);
                        if damaged {
                            // `0x00741D9F`.
                            if let Some(unit) = self.substrate.entities.get_mut(id) {
                                unit.pending_entry.install(building);
                            }
                        } else {
                            crate::sim::radio::transmit_to_contact(
                                self,
                                id,
                                crate::sim::radio::RadioMessage::Break,
                                Some(rules),
                                frame_effects,
                            );
                        }
                        destination = None;
                    }
                } else {
                    // `0x00741DCF..0x00741DDA`: DOCKING, with the NavCom
                    // saved first.
                    let nav_before = self
                        .substrate
                        .entities
                        .get(id)
                        .and_then(|unit| unit.navigation.nav_com());
                    if crate::sim::radio::transmit(
                        self,
                        id,
                        building,
                        crate::sim::radio::RadioMessage::CanDock,
                        crate::sim::radio::RadioPayload::default(),
                        Some(rules),
                        frame_effects,
                    ) != crate::sim::radio::RadioResponse::Roger
                    {
                        // `0x00741DEC`, and a depot's ArchiveTarget at
                        // `0x00741E09`.
                        crate::sim::radio::transmit_to_contact(
                            self,
                            id,
                            crate::sim::radio::RadioMessage::Break,
                            Some(rules),
                            frame_effects,
                        );
                        if unit_repair && let Some(unit) = self.substrate.entities.get_mut(id) {
                            unit.set_archive_target(Some(crate::sim::combat::TargetKind::Entity(
                                building,
                            )));
                        }
                    } else if dock_unload {
                        // `0x00741E10..0x00741E2A`: the refinery's MOVE_HERE
                        // has already run this setter for its pad; the call
                        // carries on with the NavCom that set.
                        let nav_after = self
                            .substrate
                            .entities
                            .get(id)
                            .and_then(|unit| unit.navigation.nav_com());
                        if nav_after != nav_before {
                            destination = nav_after;
                        }
                    }
                }
            }
        }
        // Native7422F4/7423CA clears NavQueue after the Enter arm, before
        // the UnitRepair tail. A false flag preserves non-NULL requests for
        // Enter's pop; the modified-NULL arm7423CA clears the queue regardless.
        if (clear_queue || destination.is_none())
            && let Some(unit) = self.substrate.entities.get_mut(id)
        {
            unit.navigation.clear_nav_queue();
        }
        // Native runs this UnitRepair tail (`0x00742C14..0x00742D3F`) after
        // the Teleporter arm (`0x007423CD..0x00742C14`), which VERA runs
        // below. The order changes nothing the arm decides: its one
        // destination-dependent branch needs a Cell, and a depot target, like
        // the NULL this tail may leave, is not one.
        if let Some((depot, true, _)) = enter_building {
            let held_by_other = self.substrate.entities.get(depot).is_some_and(|building| {
                !building.radio_contacts.is_empty() && building.radio_contacts.slot(0) != Some(id)
            });
            if held_by_other {
                if let Some(unit) = self.substrate.entities.get_mut(id) {
                    unit.set_archive_target(
                        destination.map(|_| crate::sim::combat::TargetKind::Entity(depot)),
                    );
                }
            } else if crate::sim::radio::transmit(
                self,
                id,
                depot,
                crate::sim::radio::RadioMessage::Hello,
                crate::sim::radio::RadioPayload::default(),
                Some(rules),
                frame_effects,
            ) == crate::sim::radio::RadioResponse::Roger
            {
                if crate::sim::radio::transmit_to_contact(
                    self,
                    id,
                    crate::sim::radio::RadioMessage::CanDock,
                    Some(rules),
                    frame_effects,
                ) != crate::sim::radio::RadioResponse::Roger
                {
                    crate::sim::radio::transmit_to_contact(
                        self,
                        id,
                        crate::sim::radio::RadioMessage::Break,
                        Some(rules),
                        frame_effects,
                    );
                    destination = None;
                } else {
                    depot_fast_return = true;
                }
            }
        }
        let Some(requested) = destination else {
            if teleporter {
                let _ = self.unit_teleporter_arm(id, None, rules);
            }
            if self.unit_factory_exit_arm(id, None, Some(rules), None, frame_effects) {
                return true;
            }
            self.unit_destination_power_on(id, Some(rules));
            self.foot_null_destination(id, Some(rules), None, frame_effects);
            return true;
        };
        // 7424B1..7424F0 casts the requested receiver to CellClass. A Foot
        // or Building target is not a Cell even when it stands on a dock.
        let requested_cell = match requested {
            NavTargetRef::Cell { rx, ry } => Some((rx, ry)),
            _ => None,
        };
        let skip_move_to = teleporter && self.unit_teleporter_arm(id, requested_cell, rules);
        if self.unit_factory_exit_arm(id, Some(requested), Some(rules), None, frame_effects) {
            return true;
        }
        let requested = match original_building.filter(|_| !depot_fast_return) {
            Some(building) => {
                match self.unit_dock_arm(id, building, requested, rules, frame_effects) {
                    UnitDockArm::Keep => requested,
                    UnitDockArm::Cell((rx, ry)) => NavTargetRef::Cell { rx, ry },
                    UnitDockArm::Null => {
                        self.unit_destination_power_on(id, Some(rules));
                        self.foot_null_destination(id, Some(rules), None, frame_effects);
                        return true;
                    }
                }
            }
            None => requested,
        };
        if !depot_fast_return {
            self.unit_destination_power_on(id, Some(rules));
        }
        if !self.begin_foot_destination(id, true, Some(rules), None, frame_effects) {
            if depot_fast_return && let Some(actor) = self.substrate.entities.get_mut(id) {
                actor.navigation.path_replay.clear_live_head();
            }
            return false;
        }
        let frame = self.session.binary_frame;
        let timing = super::DestinationTiming::from_rules(frame, Some(rules));
        let coord = (!skip_move_to).then(|| {
            super::navcom::nav_target_coordinate(
                requested,
                Some(id),
                self,
                self.resolved_terrain.as_ref(),
                Some((rules, &self.interner)),
            )
            .unwrap_or_else(|cause| panic!("Unit destination for {id}: {cause}"))
        });
        let jumpjet_accepted = if skip_move_to {
            None
        } else if jumpjet {
            let actor = self.substrate.entities.get_mut(id).expect("same Jumpjet");
            if super::air_movement::fly_coordinate_admitted(actor)
                && self.resolved_terrain.is_some()
            {
                super::navcom::publish_nav_com(actor, requested);
                let accepted = self
                    .jumpjet_move_to(id, coord.expect("captured destination +4C"), Some(rules))
                    .is_some();
                Some(accepted)
            } else {
                Some(false)
            }
        } else {
            None
        };
        let terrain = self.resolved_terrain.as_ref();
        let actor = self
            .substrate
            .entities
            .get_mut(id)
            .expect("same setter actor");
        let accepted = if let Some(accepted) = jumpjet_accepted {
            accepted
        } else if skip_move_to {
            super::navcom::publish_nav_com(actor, requested);
            true
        } else {
            match actor.locomotor.as_ref().map(|loco| loco.active_kind()) {
                Some(
                    LocomotorKind::Drive
                    | LocomotorKind::Ship
                    | LocomotorKind::Hover
                    | LocomotorKind::Walk,
                ) => {
                    let coord = coord.expect("accepted Move_To captures target +4C");
                    super::navcom::set_destination_internal_coord(
                        actor, requested, coord, terrain, frame,
                    );
                    true
                }
                Some(LocomotorKind::Teleport) => {
                    super::navcom::publish_nav_com(actor, requested);
                    // A Teleporter's arm keeps its Teleport only for a Cell;
                    // the Chrono Warp's Teleport takes any destination's
                    // coordinate (`0x004D9628`).
                    let coord = coord.expect("accepted Move_To captures target +4C");
                    self.teleport_move_to(
                        id,
                        coord,
                        rules,
                        is_harvester,
                        frame_effects.overlay_registry(),
                        frame_effects,
                    )
                    .unwrap_or_else(|error| {
                        log::debug!("Unit Teleport MoveTo {id}: {error}");
                        false
                    })
                }
                _ => false,
            }
        };
        // 0x004D96C2..0x004D9707: +6B7 and the +640/+668 restarts follow the
        // Move_To (or its skip) whatever it answered.
        let actor = self
            .substrate
            .entities
            .get_mut(id)
            .expect("same setter actor");
        timing.accept(actor);
        // The successful depot tail742D0B calls Foot before its
        // unconditional first path-word clear742D11 and early return.
        if depot_fast_return {
            actor.navigation.path_replay.clear_live_head();
        }
        accepted
    }

    /// The Unit setter's Dock arm (`0x00742E47..0x0074312A`): a `Harvester=`
    /// or `Weeder=` Unit (`Type+0xE0E`/`+0xE0F`) whose original destination is
    /// a building of its `Dock=` list, on any mission (vt+0x184) but Unload.
    /// Unless it is already that building's contact, the building is busy,
    /// its HELLO (`0x00742EF7`) is refused or the mission is Enter or Harvest,
    /// the miner queues Enter (`0x00742F37`) and the destination becomes
    /// NULL. Otherwise a destination that is still a building other than the
    /// contact becomes the nearby passable cell around that dock's Location
    /// cell (`0x00742FE9..0x00743123`), or NULL when there is none.
    fn unit_dock_arm(
        &mut self,
        id: u64,
        dock: u64,
        current: NavTargetRef,
        rules: &RuleSet,
        frame_effects: FrameEffects<'_>,
    ) -> UnitDockArm {
        let Some(actor) = self.substrate.entities.get(id) else {
            return UnitDockArm::Keep;
        };
        let Some(building) = self.substrate.entities.get(dock) else {
            return UnitDockArm::Keep;
        };
        let Some(object) = self.object_type(actor.type_ref(), rules) else {
            return UnitDockArm::Keep;
        };
        if actor.category() != EntityCategory::Unit
            || !(object.harvester || object.weeder)
            || !rules.harvester_can_dock_at(
                self.interner.resolve(actor.type_ref()),
                self.interner.resolve(building.type_ref()),
            )
            || actor.mission.effective().known() == Some(MissionType::Unload)
        {
            return UnitDockArm::Keep;
        }
        let movement_zone = object.movement_zone;
        let dock_cell = (building.position.rx(), building.position.ry());
        let mission_is_enter_or_harvest = |sim: &Self| {
            sim.substrate.entities.get(id).is_some_and(|actor| {
                matches!(
                    actor.mission.effective().known(),
                    Some(MissionType::Enter | MissionType::Harvest)
                )
            })
        };
        let contact = actor.radio_contacts.slot(0) == Some(dock);
        let busy = !building.radio_contacts.is_empty();
        let queue_near = contact
            || busy
            || crate::sim::radio::transmit(
                self,
                id,
                dock,
                crate::sim::radio::RadioMessage::Hello,
                crate::sim::radio::RadioPayload::default(),
                Some(rules),
                frame_effects,
            ) != crate::sim::radio::RadioResponse::Roger
            || mission_is_enter_or_harvest(self);
        if !queue_near {
            let now = self.session.binary_frame;
            let _ = self.mission_queue_exact(
                id,
                crate::sim::mission::MissionId::from_known(MissionType::Enter),
                0,
                now,
                &crate::sim::mission::authority::EntityReadyInputProvider,
            );
            return UnitDockArm::Null;
        }
        let current_building = match current {
            NavTargetRef::Object { id: target } => self
                .substrate
                .entities
                .get(target)
                .filter(|target| target.category() == EntityCategory::Structure)
                .map(|_| target),
            _ => None,
        };
        let Some(current_building) = current_building else {
            return UnitDockArm::Keep;
        };
        if self
            .substrate
            .entities
            .get(id)
            .and_then(|actor| actor.radio_contacts.slot(0))
            == Some(current_building)
        {
            return UnitDockArm::Keep;
        }
        match self.nearby_wheel_cell(
            dock_cell,
            movement_zone,
            crate::sim::find_nearby_cell::NearbySearchOptions::default(),
        ) {
            Some(cell) => UnitDockArm::Cell(cell),
            None => UnitDockArm::Null,
        }
    }

    /// Unit741970's shared tail742F48..74314C, before Foot4D94B0.
    /// An unpowered locomotor powers on when the raw Object5F6960 current
    /// cell has a UnitRepair/Bunker building on its ground list and lacks
    /// the high-bridge flag0x100. House53A130 returns false in active YR.
    /// This is independent of contacts and the requested destination; the
    /// successful depot handshake742D24 and earlier setter guards skip it.
    /// Original executable controls: building_repair.depot_service.{json,md}.
    fn unit_destination_power_on(&mut self, id: u64, rules: Option<&RuleSet>) {
        let Some(actor) = self.substrate.entities.get(id) else {
            return;
        };
        if !actor
            .locomotor
            .as_ref()
            .is_some_and(|loco| !loco.is_powered())
        {
            return;
        }
        let Some(terrain) = self.resolved_terrain.as_ref() else {
            return;
        };
        let cells = NativeCellQuery::canonical(terrain);
        let cell = ground_pose::query_object_cell(
            &cells,
            ground_pose::position_world_coord(&actor.position),
        );
        if cells.flags(cell) & 0x100 != 0 {
            return;
        }
        // The canonical dummy currently owns no ground object list. Do not
        // select a real cell's members through its mutable fallback coordinate.
        if cell == crate::map::cell_index::NativeCellIdentity::Dummy {
            return;
        }
        let (rx, ry) = cells.coord(cell);
        let service_building = self
            .cell_objects((rx as u16, ry as u16), MovementLayer::Ground)
            .any(|member| {
                let crate::sim::occupancy::CellObjectMember::Entity(candidate) = member else {
                    return false;
                };
                candidate != id
                    && self
                        .substrate
                        .entities
                        .get(candidate)
                        .is_some_and(|building| {
                            building.category() == EntityCategory::Structure
                                && rules
                                    .and_then(|rules| self.object_type(building.type_ref(), rules))
                                    .is_some_and(|object| object.unit_repair || object.bunker)
                        })
            });
        if service_building
            && let Some(loco) = self
                .substrate
                .entities
                .get_mut(id)
                .and_then(|actor| actor.locomotor.as_mut())
        {
            // ILoco+58 -> PowerOn55A8F0 writes the same power flag as the
            // explicit depot state1 release, then re-reads it via55A930.
            loco.power_on();
        }
    }

    /// The Teleporter arm of Unit Assign_Destination
    /// (`0x007423CD..0x007427C0`) for a `Teleporter=` type; any other receiver
    /// is left alone. The Teleport
    /// primary stays in charge only for a Cell destination holding no Unit
    /// while radio slot 0 holds a `DockUnload=` building (any such cell, not
    /// only the pad: oracle row `contact_other_cell`); in the dock chain that
    /// is the refinery's MOVE_HERE to its pad. Every other destination, NULL
    /// included, drives: a Drive piggybacks over whatever is active, after an
    /// active piggyback that may end has ended
    /// ([`locomotor_owner::begin_drive_for_teleporter`](super::locomotor_owner::begin_drive_for_teleporter),
    /// `0x007425E6..0x0074277E`).
    ///
    /// Back to Teleport, a Drive piggyback ends when `Is_Ok_To_End` allows it
    /// (`0x00742500..0x0074258A`). A Drive that cannot end yet is stopped, the
    /// mission set to none with Enter queued and Techno+0x1F8 raised
    /// (`0x0074258C..0x007425C6`); the FootClass::AI tail ends the stopped
    /// Drive and the Enter redispatch's re-assign warps.
    ///
    /// Returns Foot+0x6AC, the can't-end branch's other byte: the Foot tail
    /// keeps NavCom without a Move_To and clears it (`0x004D9607`). Both
    /// tails the arm reaches consume it in the same call (`0x00742D0B`,
    /// `0x00743161`), so VERA never stores it. Natively the WeaponsFactory
    /// arm's returns ([`Self::unit_factory_exit_arm`], `0x00742E37`,
    /// `0x0074317A`) would leave it set, but that needs radio slot 0 to hold
    /// a building that is both `DockUnload=` and `WeaponsFactory=`, and no
    /// retail BuildingType is.
    ///
    /// The arm needs the Chronosphere's warp latch (Techno+0x27C), a
    /// Magnetron holding the owner (+0x2B0) and the Foot lifted byte
    /// (+0x6AD) clear.
    ///
    /// The Drive install stashes the Teleport without a Stop, so a warp the
    /// Teleport had armed waits until End_Piggyback hands it back: the
    /// Teleport step runs only while Teleport is the active locomotor
    /// (`teleport_movement::teleport_process_active`).
    pub(crate) fn unit_teleporter_arm(
        &mut self,
        id: u64,
        cell: Option<(u16, u16)>,
        rules: &RuleSet,
    ) -> bool {
        let Some(actor) = self.substrate.entities.get(id) else {
            return false;
        };
        if !teleporter_unit(self, actor, Some(rules))
            || actor.chrono_warp_latch()
            || actor.lift.source().is_some()
            || actor.lift.swap_active()
        {
            return false;
        }
        let Some(active) = actor.locomotor.as_ref().map(|loco| loco.active_kind()) else {
            return false;
        };
        let dock_contact = actor
            .radio_contacts
            .slot(0)
            .and_then(|contact| self.substrate.entities.get(contact))
            .filter(|contact| contact.category() == EntityCategory::Structure)
            .and_then(|contact| self.object_type(contact.type_ref(), rules))
            .is_some_and(|object| object.dock_unload);
        // 0x007424D7: CellClass::Get_Unit(0) on the ground object list.
        let pad = dock_contact
            && cell.is_some_and(|(x, y)| {
                self.substrate
                    .occupancy
                    .first_category_on_layer(
                        x,
                        y,
                        MovementLayer::Ground,
                        EntityCategory::Unit,
                        &self.substrate.entities,
                    )
                    .is_none()
            });
        let frame = self.session.binary_frame;
        if pad {
            if active == LocomotorKind::Teleport {
                return false;
            }
            // 0x0074250E..0x0074258A: a piggyback that may end ends instead.
            if self.try_end_piggyback(id, Some(rules)) {
                return false;
            }
            let actor = self.substrate.entities.get_mut(id).expect("same arm actor");
            super::navcom::track_stop_moving(actor);
            actor.setter_force_reassign.raised = true;
            let _ = self.mission_assign_exact(id, crate::sim::mission::MissionId::NONE, frame);
            let _ = self.mission_queue_exact(
                id,
                crate::sim::mission::MissionId::from_known(MissionType::Enter),
                0,
                frame,
                &crate::sim::mission::authority::EntityReadyInputProvider,
            );
            return true;
        }
        if active != LocomotorKind::Drive
            && let Some(actor) = self.substrate.entities.get_mut(id)
        {
            super::locomotor_owner::begin_drive_for_teleporter(actor, frame);
        }
        false
    }

    /// Unit 0x741970(NULL, 1), the null destination of every Unit, as
    /// Find_Path's failure continuation (0x4D413A), the Process continuations
    /// and [`Self::assign_null_destination`] call it.
    /// - 0x741A80..0x741A9C: without a NavCom it returns before any write
    ///   unless the Techno+0x1F8 override is up; the call then clears it.
    /// - The WeaponsFactory arm ([`Self::unit_factory_exit_arm`]): a Unit
    ///   still leaving its war factory only empties its navigation list and
    ///   NavQueue and keeps NavCom.
    /// - 0x741E88: the live path word is cleared unless an Enter mission lacks
    ///   a radio contact; 0x7423BE clears NavQueue; 0x74314F enters Foot's
    ///   null arm ([`Self::foot_null_destination`]), whose locomotor Stop
    ///   keeps a Drive/Ship head and re-targets a moving Jumpjet to the cell
    ///   under it.
    ///
    /// - A `Teleporter=` type runs its arm first ([`Self::unit_teleporter_arm`]):
    ///   a NULL destination installs a Drive over the Teleport, which the
    ///   FootClass::AI tail ends again once it is stopped.
    /// - The BalloonHover arm runs ahead of all of these
    ///   ([`Self::balloon_hover_keeps_nav_com`]).
    ///
    /// A Jumpjet Stop's failed search retains the caller's overlay context
    /// for synchronous damage; callers without that context retain their
    /// existing damage-closure limitation.
    /// Returns whether Foot 0x4D94B0 ran.
    pub(crate) fn set_unit_null_destination(
        &mut self,
        id: u64,
        rules: Option<&RuleSet>,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> bool {
        if rules.is_some_and(|rules| self.balloon_hover_keeps_nav_com(id, rules)) {
            return false;
        }
        let Some(actor) = self.substrate.entities.get(id) else {
            return false;
        };
        if actor.navigation.nav_com().is_none() && !actor.setter_force_reassign.raised {
            return false;
        }
        let teleporter = teleporter_unit(self, actor, rules);
        if let Some(actor) = self.substrate.entities.get_mut(id) {
            actor.setter_force_reassign.raised = false;
            //741A96..741ACD; same native owner as the non-null setter.
            if deployment_refuses_destination(actor) {
                actor.navigation.stop_moving();
                return false;
            }
        }
        if teleporter && let Some(rules) = rules {
            let _ = self.unit_teleporter_arm(id, None, rules);
        }
        let Some(actor) = self.substrate.entities.get_mut(id) else {
            return false;
        };
        super::movement_commands::clear_destination_path_head(actor);
        if self.unit_factory_exit_arm(id, None, rules, registry, frame_effects) {
            return false;
        }
        if let Some(actor) = self.substrate.entities.get_mut(id) {
            actor.navigation.clear_nav_queue();
        }
        self.unit_destination_power_on(id, rules);
        self.foot_null_destination(id, rules, registry, frame_effects);
        true
    }

    /// The WeaponsFactory arm of Unit 0x741970 (`0x00742D46..0x00742E41`),
    /// after the Teleporter arm and the UnitRepair tail, before the Dock arm.
    /// It applies to a Unit whose radio contact 0 is a `WeaponsFactory=`
    /// building ([`Self::is_weapons_factory`], the factory it is still
    /// leaving), on any committed mission but Enter (`+0xAC`).
    ///
    /// The Unit keeps its NavCom; a Drive product follows its exit track and
    /// has none. Unless `destination` is the factory's exit Cell
    /// ([`war_factory_exit_cell`](super::war_factory_exit_cell),
    /// `0x00742D98..0x00742DFD`):
    /// - its navigation list and NavQueue empty (`0x00742DFF`, `0x00742E0E`);
    /// - a destination joins the list ([`Self::queue_navigation_list`],
    ///   `0x00742E28`) for the Unit's next idle;
    /// - the setter returns (`0x00742E37`, `0x0074317A`).
    ///
    /// A Unit a Magnetron holds (`+0x2B0`) sends its contact OVER_OUT instead
    /// (vt+0x274(3), `0x00742E3A..0x00742E41`) and the setter goes on.
    ///
    /// An order given while the factory door opens, with the product inside on
    /// Guard, gets the same treatment. Readiness then commences its queued Move
    /// despite the factory contact, and Mission_Move's idle entry runs. At a
    /// height-0 Cell centre the Unit breaks the link (Enter_Idle_Mode's OVER_OUT
    /// arm) and Handle_Navigation_List sends it at once. Anywhere else,
    /// Handle's send re-enters this arm, and its RemoveIndex (`0x004DA068`)
    /// drops the order.
    ///
    /// Returns whether the setter returns here.
    fn unit_factory_exit_arm(
        &mut self,
        id: u64,
        destination: Option<NavTargetRef>,
        rules: Option<&RuleSet>,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> bool {
        let (Some(rules), Some(actor)) = (rules, self.substrate.entities.get(id)) else {
            return false;
        };
        let Some(factory) = actor
            .radio_contacts
            .slot(0)
            .filter(|&contact| self.is_weapons_factory(contact, rules))
            .and_then(|contact| self.substrate.entities.get(contact))
        else {
            return false;
        };
        if actor.mission.current().raw() == 7 {
            return false;
        }
        if actor.lift.source().is_some() {
            crate::sim::radio::transmit_to_contact(
                self,
                id,
                crate::sim::radio::RadioMessage::Break,
                Some(rules),
                frame_effects,
            );
            return false;
        }
        let (rx, ry) =
            super::war_factory_exit_cell(ground_pose::position_world_coord(&factory.position));
        if destination == Some(NavTargetRef::Cell { rx, ry }) {
            return false;
        }
        let actor = self
            .substrate
            .entities
            .get_mut(id)
            .expect("same setter actor");
        actor.navigation.clear_nav_list();
        actor.navigation.clear_nav_queue();
        if let Some(destination) = destination {
            self.queue_navigation_list(id, destination, rules, registry, frame_effects);
        }
        true
    }

    /// Unit 0x741970's BalloonHover arm (0x741983..0x741A7D), the first test
    /// of a NULL destination. A `BalloonHover=` type (+0xD6A) holding a
    /// NavCom (+0x5A4) and a Target (+0x2B4) keeps the NavCom when it:
    /// - is the Target;
    /// - is a Cell whose first ground object (+0xE4) is the Target; or
    /// - lies within a quarter turn of the body: Direction_To(Target)
    ///   (0x5F3DB0) minus PrimaryFacing's Current (+0x388), as a signed word,
    ///   is at most 0x4000 either way (0x4D03D0), so 0x8000 is outside.
    ///
    /// A kept NavCom puts the Unit on Attack (Assign_Mission, vt+0x1F0
    /// 0x5B2FD0) unless Get_Mission (vt+0x184 0x5B3040, current else queued)
    /// already answers Attack, and the setter returns with nothing else
    /// written. Event STOP therefore calls the setter again for a balloon once
    /// it has cleared the Target.
    ///
    /// Native comparison: tools/spatial_oracle/balloon_hover.json `null` rows.
    pub(crate) fn balloon_hover_keeps_nav_com(&mut self, id: u64, rules: &RuleSet) -> bool {
        let Some(actor) = self.substrate.entities.get(id) else {
            return false;
        };
        let (Some(nav_com), Some(target)) =
            (actor.navigation.nav_com(), actor.attack_target.target())
        else {
            return false;
        };
        if !self
            .object_type(actor.type_ref(), rules)
            .is_some_and(|object| object.balloon_hover)
        {
            return false;
        }
        let frame = self.session.binary_frame;
        let leads_to_target = match (nav_com, target) {
            (_, crate::sim::combat::TargetKind::Cell(rx, ry)) => {
                nav_com == NavTargetRef::cell(rx, ry)
            }
            (NavTargetRef::Cell { rx, ry }, crate::sim::combat::TargetKind::Entity(target)) => {
                self.substrate
                    .occupancy
                    .get(rx, ry)
                    .and_then(|cell| cell.first_on_layer(MovementLayer::Ground))
                    == Some(target)
            }
            (_, crate::sim::combat::TargetKind::Entity(target)) => {
                nav_com == NavTargetRef::object(target)
            }
        };
        let keeps = leads_to_target
            || super::turret::facing_toward_target(actor, &target, self).is_some_and(|direction| {
                let turn = direction.wrapping_sub(actor.body_facing_current(frame)) as i16;
                i32::from(turn).abs() <= 0x4000
            });
        if !keeps {
            return false;
        }
        let attack = crate::sim::mission::MissionId::from_known(MissionType::Attack);
        if actor.mission.effective() != attack {
            let _ = self.mission_assign_exact(id, attack, frame);
        }
        true
    }
}
