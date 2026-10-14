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
//! 0x4D41C2 relocated a nonhuman Foot (0x500200, unported; see foot_path).
//! The ladder is ported because native reaches it after that relocation.
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

/// Foot+64C after a found route (0x4B3285 / 0x6A28D5).
const FOUND_ROUTE_RETRIES: u32 = 10;

/// A Unit on Drive or Ship, whose class setter is Unit 0x741970.
fn track_unit(entity: &GameEntity) -> bool {
    entity.category == EntityCategory::Unit
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
    entity.category == EntityCategory::Unit
        && entity
            .locomotor
            .as_ref()
            .is_some_and(|loco| loco.active_kind() == LocomotorKind::Hover)
}

/// A Unit on a Walk locomotor (no retail VehicleType; a map may assign one):
/// the Unit setter's Foot tail reaches Walk Move_To (0x0075ACB0).
fn walk_unit(entity: &GameEntity) -> bool {
    entity.category == EntityCategory::Unit
        && entity
            .locomotor
            .as_ref()
            .is_some_and(|loco| loco.active_kind() == LocomotorKind::Walk)
}

/// A Unit on a Teleport locomotor, which for a type that is no `Teleporter=`
/// is the Chrono Warp's (`SuperClass::Launch @ 0x006CCB4A`): the Unit
/// setter's Foot tail reaches Teleport Move_To (`0x00718100`).
fn teleport_unit(entity: &GameEntity) -> bool {
    entity.category == EntityCategory::Unit
        && entity
            .locomotor
            .as_ref()
            .is_some_and(|loco| loco.active_kind() == LocomotorKind::Teleport)
}

/// A Unit on a Jumpjet locomotor: its null destination reaches the Jumpjet's
/// `Stop_Moving` through the same Unit setter.
fn jumpjet_unit(entity: &GameEntity) -> bool {
    entity.category == EntityCategory::Unit
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
    entity.category == EntityCategory::Unit
        && rules
            .and_then(|rules| sim.object_type(entity.type_ref(), rules))
            .is_some_and(|object| object.teleporter)
}

/// `0x00741A96..0x00741ACD`: Unit Set_Destination refuses while the Unit
/// deploys either way (`+0x6E1`, `+0x6E2`), and while it is deployed
/// (`+0x6E0`) unless a Magnetron holds it (`+0x2B0`, `0x00741AC3`).
fn deployment_refuses_destination(actor: &GameEntity) -> bool {
    match actor.mission_leaf.as_unit() {
        Some(leaf) if leaf.deployed() != 0 => actor.locomotor_source.is_none(),
        _ => actor.is_deployed(),
    }
}

pub(super) fn track_destination(entity: &GameEntity) -> Option<DriveCoord> {
    let loco = entity.locomotor.as_ref()?;
    loco.track_destination(super::track_process::TrackFamily::from_kind(loco.kind)?)
}

/// `if (head != Null) { head = Null; +63 = 0; }` as every arm here writes it.
/// Raw occupation marks are untouched, as in native.
pub(super) fn clear_track_head(entity: &mut GameEntity) {
    if let Some(loco) = entity.locomotor.as_mut()
        && let Some(family) = super::track_process::TrackFamily::from_kind(loco.kind)
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
            .retries_left = FOUND_ROUTE_RETRIES;
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
        let probe = destination.unwrap_or(DriveCoord { x: 0, y: 0, z: 0 });
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
        if actor.navigation.path_runtime.retries_left > 0 {
            actor.navigation.path_runtime.retries_left -= 1;
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
        let target = self
            .substrate
            .entities
            .get(id)?
            .attack_target
            .as_ref()?
            .target;
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
            actor.mark_stopped_cannot_fire();
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
            && let Some(family) = super::track_process::TrackFamily::from_kind(loco.kind)
            && let Some(mut progress) = loco.track_progress(family)
        {
            progress.turn_index = -1;
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
        let height = ground_pose::query_object_cell_height(&cells, location, actor.on_bridge);
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
        let destination = track_destination(actor).unwrap_or(DriveCoord { x: 0, y: 0, z: 0 });
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
        if actor.navigation.nav_queue.is_empty() {
            self.set_unit_null_destination(id, Some(rules), None, frame_effects);
            return false;
        }
        super::navcom::foot_stop_moving(actor);
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
        if actor.navigation.nav_queue.is_empty() {
            self.set_unit_null_destination(id, rules, None, frame_effects);
        } else {
            super::navcom::foot_stop_moving(actor);
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
            .is_some_and(|e| e.lifecycle.object_alive)
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
        if !actor.navigation.pending_destination_setter {
            return;
        }
        actor.navigation.pending_destination_setter = false;
        // A NavCom cleared after the restore (a Stop) leaves nothing owed.
        let Some(requested) = actor.navigation.nav_com else {
            return;
        };
        if !self.destination_setter_available(id, requested, rules, registry) {
            return;
        }
        if let Some(actor) = self.substrate.entities.get_mut(id) {
            super::navcom::foot_stop_moving(actor);
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
        let Some(category) = self.substrate.entities.get(id).map(|actor| actor.category) else {
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
        if super::navcom::nav_targets_same_receiver(actor.navigation.nav_com, requested)
            && !actor.setter_force_reassign
        {
            return true;
        }
        let actor = self
            .substrate
            .entities
            .get_mut(id)
            .expect("same Unit setter");
        actor.setter_force_reassign = false;
        //741A96..741ACD, after the same-NavCom guard: deployment refuses
        // through Foot4DF0D0, which clears NavCom, not locomotor movement.
        if deployment_refuses_destination(actor) {
            super::navcom::foot_stop_moving(actor);
            return false;
        }
        let is_harvester = self
            .substrate
            .entities
            .get(id)
            .and_then(|actor| self.object_type(actor.type_ref(), rules))
            .is_some_and(|object| object.harvester);
        {
            let actor = self
                .substrate
                .entities
                .get_mut(id)
                .expect("same setter actor");
            super::movement_commands::clear_destination_path_head(actor);
        }
        // Unit741C4F..741E8E and742C14..742D0B: the UnitRepair
        // Building arm belongs to this same class setter, including callers
        // that already HELLOed in Foot70D7E0. No caller repeats its handshake.
        // The original incoming Building identity survives a modified NULL
        // argument. Native controls: refinery_dock.json; source741970.
        let depot = match requested {
            NavTargetRef::Building { id }
            | NavTargetRef::Entity { id }
            | NavTargetRef::Object { id } => self
                .substrate
                .entities
                .get(id)
                .filter(|building| building.category == EntityCategory::Structure)
                .filter(|building| {
                    self.object_type(building.type_ref(), rules)
                        .is_some_and(|object| object.unit_repair)
                })
                .map(|_| id),
            _ => None,
        };
        let mut destination = Some(requested);
        let mut depot_fast_return = false;
        if let Some(depot) = depot {
            let enter_without_contact = self.substrate.entities.get(id).is_some_and(|unit| {
                (unit.mission.effective().known() == Some(MissionType::Enter)
                    || unit.mission.queued().known() == Some(MissionType::Enter))
                    && unit.radio_contacts.is_empty()
            });
            if enter_without_contact {
                let busy = self
                    .substrate
                    .entities
                    .get(depot)
                    .is_some_and(|building| !building.radio_contacts.is_empty());
                if busy {
                    if let Some(unit) = self.substrate.entities.get_mut(id) {
                        unit.set_archive_target(Some(crate::sim::combat::TargetKind::Entity(
                            depot,
                        )));
                    }
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
                        crate::sim::docking::building_dock::set_pending_entry(
                            self,
                            id,
                            Some(depot),
                        );
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
                } else if crate::sim::radio::transmit(
                    self,
                    id,
                    depot,
                    crate::sim::radio::RadioMessage::CanDock,
                    crate::sim::radio::RadioPayload::default(),
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
                    if let Some(unit) = self.substrate.entities.get_mut(id) {
                        unit.set_archive_target(Some(crate::sim::combat::TargetKind::Entity(
                            depot,
                        )));
                    }
                }
            }
            // Native7422F4/7423CA clears NavQueue after the initial
            // Enter/no-contact DOCKING arm, before the UnitRepair tail.
            // A false flag preserves non-NULL requests for Enter's pop;
            // the modified-NULL arm7423CA clears the queue regardless.
            if (clear_queue || destination.is_none())
                && let Some(unit) = self.substrate.entities.get_mut(id)
            {
                unit.navigation.nav_queue.clear();
            }
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
        } else if clear_queue && let Some(unit) = self.substrate.entities.get_mut(id) {
            unit.navigation.nav_queue.clear();
        }
        let Some(requested) = destination else {
            if teleporter {
                let _ = self.unit_teleporter_arm(id, None, rules);
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
                &self.substrate.entities,
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
                    let cell =
                        requested_cell.unwrap_or(((coord.x / 256) as u16, (coord.y / 256) as u16));
                    let point = super::navcom::target_cell_coord(cell.0, cell.1, None);
                    self.teleport_move_to(id, point, rules, is_harvester, None, frame_effects)
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
                            building.category == EntityCategory::Structure
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
    /// `0x00743161`), so it is never stored.
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
            || actor.locomotor_source.is_some()
            || actor.foot_locomotor_swap_active
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
            .filter(|contact| contact.category == EntityCategory::Structure)
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
            actor.setter_force_reassign = true;
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
    /// - 0x742D46..0x742E1F: while radio slot 0 holds a WeaponsFactory
    ///   building (BuildingType+16BD, ReadINI 0x460A72) and the current
    ///   mission is not Enter, it clears only NavQueue (+588) and the +5AC
    ///   vector (not represented) and returns: the exiting unit keeps NavCom.
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
    /// Residual (not represented): a Unit a Magnetron holds (+2B0) sent to
    /// a `WeaponsFactory=` building skips that building's exit arm
    /// (0x742D8A -> 0x742E3A). Only a building firing an `IsLocomotor=`
    /// warhead could send it there, and no stock building does.
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
        if actor.navigation.nav_com.is_none() && !actor.setter_force_reassign {
            return false;
        }
        let teleporter = teleporter_unit(self, actor, rules);
        if let Some(actor) = self.substrate.entities.get_mut(id) {
            actor.setter_force_reassign = false;
            //741A96..741ACD; same native owner as the non-null setter.
            if deployment_refuses_destination(actor) {
                super::navcom::foot_stop_moving(actor);
                return false;
            }
        }
        if teleporter && let Some(rules) = rules {
            let _ = self.unit_teleporter_arm(id, None, rules);
        }
        let Some(actor) = self.substrate.entities.get(id) else {
            return false;
        };
        let factory_contact = actor
            .radio_contacts
            .slot(0)
            .and_then(|contact| self.substrate.entities.get(contact))
            .is_some_and(|contact| {
                contact.category == EntityCategory::Structure
                    && rules
                        .and_then(|rules| self.object_type(contact.type_ref(), rules))
                        .is_some_and(|kind| kind.weapons_factory)
            });
        let current_enter = actor.mission.current().raw() == 7;
        let actor = self
            .substrate
            .entities
            .get_mut(id)
            .expect("same setter actor");
        super::movement_commands::clear_destination_path_head(actor);
        if factory_contact && !current_enter {
            actor.navigation.nav_queue.clear();
            return false;
        }
        actor.navigation.nav_queue.clear();
        self.unit_destination_power_on(id, rules);
        self.foot_null_destination(id, rules, registry, frame_effects);
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
        let (Some(nav_com), Some(target)) = (
            actor.navigation.nav_com,
            actor.attack_target.as_ref().map(|attack| attack.target),
        ) else {
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
                super::navcom::nav_targets_same_receiver(
                    Some(nav_com),
                    NavTargetRef::object(target),
                )
            }
        };
        let keeps = leads_to_target
            || super::turret::facing_toward_target(actor, &target, &self.substrate.entities)
                .is_some_and(|direction| {
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
