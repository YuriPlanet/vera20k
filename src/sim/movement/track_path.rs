//! Drive/Ship `Process_Movement`'s no-queue path request and its two
//! continuations, plus the Unit class setters they reach.
//!
//! Drive 0x4B2630 and Ship 0x6A1C80 are instruction twins in these arms
//! (aligned in the track-process-path handoff R1); each Drive address below
//! is followed by its Ship twin. Order within one Process call:
//! - 0x4B281C..0x4B2845 / 0x6A1E6C..0x6A1E95: the Foot+640 exact-zero wait,
//!   owned by the mover visit (`movement_tick::no_queue_path_request`);
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
//! - Adapter-only stops. The Guard command and pursuit ClearMovement drop only
//!   the scheduling adapter and leave NavCom, +34 and path words; the track
//!   terminal defers the order and the same-call continuation finishes it
//!   toward NavCom (before the continuation, the next frame did). Their
//!   native null-destination payloads are unverified. Effect: the unit
//!   resumes its order after the track instead of stopping. Callers of the
//!   class setter vt+0x480(NULL, 1) take the Unit setter instead
//!   (`assign_null_destination`).

use super::foot_path::FootPathOutcome;
use super::ground_pose;
use super::infantry_entry::InfantryEntryArgs;
use super::movement_tick::FootPathRequest;
use crate::map::entities::EntityCategory;
use crate::map::overlay_types::OverlayTypeRegistry;
use crate::map::resolved_terrain::NativeCellQuery;
use crate::rules::locomotor_type::LocomotorKind;
use crate::rules::mission_data::MissionType;
use crate::rules::ruleset::RuleSet;
use crate::sim::cell_kernel::native_coord_distance;
use crate::sim::components::{DriveCoord, NavTargetRef};
use crate::sim::game_entity::GameEntity;
use crate::sim::movement::block_index::LentOwnerBlockSet;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::pathfinding::PathGrid;
use crate::sim::world::Simulation;
use crate::util::direction::DIRECTION_DELTAS;
use crate::util::fixed_math::SimFixed;
use crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS;

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

pub(super) fn track_destination(entity: &GameEntity) -> Option<DriveCoord> {
    match entity.locomotor.as_ref()?.kind {
        LocomotorKind::Drive => entity.drive_locomotion.as_ref()?.destination,
        LocomotorKind::Ship => entity.ship_locomotion.as_ref()?.destination,
        _ => None,
    }
}

/// `if (head != Null) { head = Null; +63 = 0; }` as every arm here writes it.
/// Raw occupation marks are untouched, as in native.
pub(super) fn clear_track_head(entity: &mut GameEntity) {
    match entity.locomotor.as_ref().map(|loco| loco.kind) {
        Some(LocomotorKind::Drive) => {
            if let Some(drive) = entity.drive_locomotion.as_mut()
                && drive.head_to.take().is_some()
            {
                drive.track_valid = false;
            }
        }
        Some(LocomotorKind::Ship) => {
            if let Some(ship) = entity.ship_locomotion.as_mut()
                && ship.head_to.take().is_some()
            {
                ship.track_valid = false;
            }
        }
        _ => {}
    }
}

/// `((Current >> 12) + 1) >> 1 & 7` over the body FacingClass (0x4B2A88..94).
fn facing_octant(entity: &GameEntity, frame: u32) -> u8 {
    let facing = entity
        .body_facing
        .as_ref()
        .map_or(u16::from(entity.facing) << 8, |f| f.current(frame));
    ((((facing >> 12) + 1) >> 1) & 7) as u8
}

impl Simulation {
    /// Drive4B28A3 / Ship6A1EF3 and the continuation that follows it.
    pub(crate) fn run_track_path_request(
        &mut self,
        request: &FootPathRequest,
        lent: Option<&mut LentOwnerBlockSet>,
        rules: Option<&RuleSet>,
        fallback: Option<&PathGrid>,
        registry: Option<&OverlayTypeRegistry>,
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
        let found = self.foot_find_path(request, lent, rules, fallback, registry)?;
        self.continue_track_path_request(id, found, rules, registry)
    }

    /// The continuation after `Find_Path` returned (0x4B28A8 / 0x6A1EF8);
    /// tools/spatial_oracle/track_path_continuation supplies its result.
    pub(crate) fn continue_track_path_request(
        &mut self,
        id: u64,
        found: super::foot_path::FindPathResult,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) -> Result<FootPathOutcome, String> {
        use super::foot_path::FindPathResult;
        //4B28B3..4B28C7 / 6A1F03..6A1F17: a vanished Foot sets the out byte.
        if self.substrate.entities.get(id).is_none() {
            return Ok(FootPathOutcome::Deleted);
        }
        let outcome = match found {
            FindPathResult::Route => self.finish_found_track_path(id, rules, registry)?,
            FindPathResult::EmptyRoute => {
                //Residual: a zero-cost route leaves Foot+5E0 at -1 and native
                //continues with that word (the success arm then steps toward
                //octant 7 and head selection turns toward 0xE000). VERA ends
                //the visit instead. Trigger: +34 in the mover's own cell: a
                //non-Cell NavCom whose coordinate lies there (a same-cell Cell
                //NavCom stops earlier, 0x4B066C), or a forced track's end
                //(Force_Track wrote +34 = its cell, 0x4B0D3F, and a null
                //NavCom skips the arrival clear, 0x4B2129), in the track-end
                //Process. Effect: no turn; whether native then stops or
                //re-requests is unexecuted.
                FootPathOutcome::Returned
            }
            FindPathResult::Failed => self.finish_failed_track_path(id, rules, registry)?,
        };
        if outcome == FootPathOutcome::Returned {
            self.retire_idle_track_adapter(id);
        }
        Ok(outcome)
    }

    /// 0x4B2F45..0x4B32A1 / 0x6A2595..0x6A28F1 after a found route.
    fn finish_found_track_path(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
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
        let stop = self.answer_track_ahead_cell(id, next, direction, rules, registry)?;
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
    ) -> Result<FootPathOutcome, String> {
        let destination = self.substrate.entities.get(id).and_then(track_destination);
        //4B28CD / 6A1F1D: the zone precheck on the LIVE destination, which the
        //Unit receiver may already have cleared (Cell(0,0) refuses).
        let probe = destination.unwrap_or(DriveCoord { x: 0, y: 0, z: 0 });
        if !self.foot_path_zone_precheck(id, probe, rules)? {
            self.set_unit_null_destination(id, Some(rules));
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
            && native_coord_distance(
                location.x.wrapping_sub(destination.x),
                location.y.wrapping_sub(destination.y),
                location.z.wrapping_sub(destination.z),
            ) < rules.general.close_enough
            && matches!(mission, Some(MissionType::Move | MissionType::AreaGuard))
        {
            //4B29AF..4B2A44: clear the head, then stop or take the waypoint.
            if self.stop_or_take_next_waypoint(id, rules) {
                return Ok(FootPathOutcome::Returned);
            }
            //4B2A06..4B2A11: a dead Foot returns before the tail.
            if !self.track_owner_alive(id) {
                return Ok(FootPathOutcome::Returned);
            }
            return self.finish_track_path_tail(id, rules, registry);
        }
        //4B2A47..4B2B17 / 6A2097..: the cell ahead of the body facing.
        let frame = self.session.binary_frame;
        let octant = facing_octant(actor, frame);
        let current = super::foot_path::coord_cell(location);
        let (dx, dy) = DIRECTION_DELTAS[usize::from(octant)];
        let ahead = (i32::from(current.0) + dx, i32::from(current.1) + dy);
        if self.answer_track_ahead_cell(id, ahead, octant, rules, registry)? {
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
            return self.finish_track_path_tail(id, rules, registry);
        }
        //4B2DDE..4B2E30: the exhausted ladder.
        if self.stop_or_take_next_waypoint(id, rules) {
            return Ok(FootPathOutcome::Returned);
        }
        //4B2E36..4B2E70: a dead Foot returns; otherwise the one-shot +68A
        //ScoldSound. Foot+68A has no nonzero writer in the program (R2 (d)),
        //so the sound is not represented.
        if !self.track_owner_alive(id) {
            return Ok(FootPathOutcome::Returned);
        }
        self.finish_track_path_tail(id, rules, registry)
    }

    /// 0x4B2E77..0x4B2F1A / 0x6A24C7..: a stopped Foot holding a target it
    /// cannot fire at drops it; then the head and selector are cleared.
    fn finish_track_path_tail(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) -> Result<FootPathOutcome, String> {
        let actor = self
            .substrate
            .entities
            .get(id)
            .ok_or("retired Drive/Ship path owner")?;
        let moving = super::motion_query::is_moving(actor).unwrap_or(false);
        if !moving && let Some(target) = actor.attack_target.as_ref().map(|t| t.target) {
            //4B2E92 vt+3AC = TechnoClass::CanFireAtTarget 0x6F7780.
            let can_fire = self.resolved_terrain.as_ref().is_some_and(|terrain| {
                crate::sim::combat::can_fire_at_target(
                    &self.substrate.entities,
                    rules,
                    &self.interner,
                    id,
                    &target,
                    terrain,
                    Some(&self.house_alliances),
                    &crate::sim::combat::line_of_fire::LineOfFireInputs {
                        overlay_grid: self.overlay_grid.as_ref(),
                        overlay_registry: registry,
                        alliances: Some(&self.house_alliances),
                    },
                )
            });
            if !can_fire {
                //4B2E9F sets the +688 scan latch (not represented; see
                //combat::greatest_threat's residual). The Team retarget
                //0x6EC3A0 has no production reach (no TeamClass instance).
                if self.team_script_vm.team_for_member(id).is_some() {
                    return Err(
                        "Drive/Ship tail Team retarget (0x6EC3A0) is not represented".into(),
                    );
                }
                if let Some(actor) = self.substrate.entities.get_mut(id) {
                    crate::sim::mission::concrete_effects::represented_assign_target(actor, None);
                }
            }
        }
        let actor = self
            .substrate
            .entities
            .get_mut(id)
            .ok_or("retired Drive/Ship path owner")?;
        clear_track_head(actor);
        //4B2F07: selector (+58) = -1. +61 has no reader in the program.
        match actor.locomotor.as_ref().map(|loco| loco.kind) {
            Some(LocomotorKind::Drive) => {
                if let Some(drive) = actor.drive_locomotion.as_mut() {
                    drive.track.turn_index = -1;
                }
            }
            Some(LocomotorKind::Ship) => {
                if let Some(ship) = actor.ship_locomotion.as_mut() {
                    ship.track.turn_index = -1;
                }
            }
            _ => {}
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
                let owner = self
                    .interner
                    .resolve(
                        self.substrate
                            .entities
                            .get(id)
                            .ok_or("retired Drive/Ship path owner")?
                            .owner(),
                    )
                    .to_owned();
                let _ = crate::sim::gate_runtime::request_gate_open_for_cell(
                    &mut self.substrate.entities,
                    &self.substrate.occupancy,
                    (cell.0 as u16, cell.1 as u16),
                    id,
                    &owner,
                    rules,
                    &self.house_alliances,
                    &self.interner,
                );
                Ok(false)
            }
            6 => self.answer_track_ally_cell(id, cell, rules),
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
        let blocker = crate::sim::cell_kernel::nearest_eligible_in_order(
            crate::sim::cell_kernel::CellQueryPoint { x: 0, y: 0 },
            self.substrate
                .occupancy
                .get(key.0, key.1)
                .into_iter()
                .flat_map(|list| list.iter_layer(layer))
                .filter_map(|entry| self.substrate.entities.get(entry.entity_id))
                .map(|entity| {
                    let coord = self.object_type(entity.type_ref(), rules).map_or_else(
                        || ground_pose::position_world_coord(&entity.position),
                        |kind| ground_pose::object_center_coord(entity, kind),
                    );
                    (
                        entity.stable_id(),
                        true,
                        crate::sim::cell_kernel::CellQueryPoint {
                            x: coord.x,
                            y: coord.y,
                        },
                    )
                }),
        );
        let Some(blocker) = blocker else {
            return Ok(false);
        };
        let allied = self.substrate.entities.get(blocker).is_some_and(|b| {
            crate::map::houses::are_houses_friendly(
                &self.house_alliances,
                self.interner.resolve(actor.owner()),
                self.interner.resolve(b.owner()),
            )
        });
        //4B2BF1..4B2C04: IsTrain (Type+C94) is set by no retail TechnoType.
        if !allied {
            return Ok(false);
        }
        let destination = track_destination(actor).unwrap_or(DriveCoord { x: 0, y: 0, z: 0 });
        //4B2C14..4B2C62: Distance3D (0x41C380) below CloseEnough, then no
        //radio contact (0x65AE30) and the shared stop band.
        let close = native_coord_distance(
            location.x.wrapping_sub(destination.x),
            location.y.wrapping_sub(destination.y),
            location.z.wrapping_sub(destination.z),
        ) < rules.general.close_enough;
        if close && actor.radio_contacts.is_empty() && self.track_stop_band(location, destination) {
            //4B2CDD..4B2D65: clear the head, then stop or take the waypoint.
            self.stop_or_take_next_waypoint(id, rules);
            return Ok(true);
        }
        //4B2D68..4B2DC0: the forced scatter of the refused cell.
        self.scatter_blocked_track_cell(id, (cell.0 as i16, cell.1 as i16), rules, None);
        Ok(false)
    }

    /// The shared stop/waypoint pair: clear the head; with an empty NavQueue
    /// SetDestination(NULL, 1), else Foot 0x4DF0D0 (NavCom only) then the
    /// Unit idle entry (+0x484 = 0x738970), whose AL the caller may return.
    /// Returns that AL (false after the NULL setter).
    pub(super) fn stop_or_take_next_waypoint(&mut self, id: u64, rules: &RuleSet) -> bool {
        let Some(actor) = self.substrate.entities.get_mut(id) else {
            return false;
        };
        clear_track_head(actor);
        if actor.navigation.nav_queue.is_empty() {
            self.set_unit_null_destination(id, Some(rules));
            return false;
        }
        super::navcom::foot_stop_moving(actor);
        self.track_enter_idle_mode(id, Some(rules))
    }

    /// The outer Process stop at a same-cell Cell NavCom (0x4B066C..0x4B06D2)
    /// or a Guard exact destination (0x4B06D5..0x4B0772), Ship twins
    /// 0x69FD13 / 0x69FD7F: NavQueue empty -> SetDestination(NULL, 1), else
    /// Foot 0x4DF0D0 then the Unit idle entry (+0x484). No head write.
    pub(super) fn track_navcom_stop(&mut self, id: u64, rules: Option<&RuleSet>) {
        let Some(actor) = self.substrate.entities.get_mut(id) else {
            return;
        };
        if actor.navigation.nav_queue.is_empty() {
            self.set_unit_null_destination(id, rules);
        } else {
            super::navcom::foot_stop_moving(actor);
            self.track_enter_idle_mode(id, rules);
        }
        self.retire_idle_track_adapter(id);
    }

    /// The outer Process zone drop (0x4B09EC..0x4B0A68 / 0x6A00B5..0x6A0131):
    /// clear the head, then the shared stop/waypoint pair.
    pub(super) fn track_zone_drop(&mut self, id: u64, rules: &RuleSet) {
        self.stop_or_take_next_waypoint(id, rules);
        self.retire_idle_track_adapter(id);
    }

    /// Foot+90 as Drive re-reads it after a synchronous setter.
    pub(super) fn track_owner_alive(&self, id: u64) -> bool {
        self.substrate
            .entities
            .get(id)
            .is_some_and(|e| e.lifecycle.object_alive)
    }

    /// Retire the MovementTarget scheduling adapter once the Drive/Ship
    /// locomotor has neither a destination nor a head nor an active track
    /// (native Is_Moving false, no Process_Track work left).
    fn retire_idle_track_adapter(&mut self, id: u64) {
        let Some(actor) = self.substrate.entities.get_mut(id) else {
            return;
        };
        let head = match actor.locomotor.as_ref().map(|loco| loco.kind) {
            Some(LocomotorKind::Drive) => actor.drive_locomotion.as_ref().and_then(|d| d.head_to),
            Some(LocomotorKind::Ship) => actor.ship_locomotion.as_ref().and_then(|s| s.head_to),
            _ => return,
        };
        if track_destination(actor).is_none()
            && head.is_none()
            && super::track_head::active_track_family(actor).is_none()
        {
            actor.movement_target = None;
        }
    }

    /// A Drive/Ship order that Rust deferred (the `pending_arrival_clear`
    /// flag), finished before the locomotor Process or, after a track end,
    /// before the same-call continuation's gates (`track_continuation`),
    /// where natively its setter already ran:
    /// - Enter_Idle_Mode took a NavQueue waypoint at an arrival (its true
    ///   return ended that Process, 0x4B2273): +34 and a NavCom naming the
    ///   same cell remain and only the scheduling adapter is missing; the
    ///   no-queue arm (Drive 0x4B281C / Ship 0x6A1E75) requests the route,
    ///   gated by Foot+640;
    /// - a mission restore represents `Assign_Destination(saved, 1)` (Unit
    ///   0x741970) by NavCom alone (`mission::authority`), while +34 may still
    ///   hold an older order (a pursuit cell). Native NavCom and +34 never
    ///   disagree after that setter, so a missing or disagreeing +34 completes
    ///   the class setter toward NavCom's live coordinate. Before Process the
    ///   Foot is then moving, as native, and the Guard/Unload tail
    ///   (0x4B08D1) cannot strand it;
    /// - with neither, the queued waypoint is taken through the Foot setter,
    ///   else the NULL clear.
    pub(crate) fn complete_pending_track_order(&mut self, id: u64, rules: Option<&RuleSet>) {
        let Some(actor) = self.substrate.entities.get(id) else {
            return;
        };
        let Some(kind) = actor
            .locomotor
            .as_ref()
            .map(|loco| loco.active_kind())
            .filter(|kind| matches!(kind, LocomotorKind::Drive | LocomotorKind::Ship))
        else {
            return;
        };
        if !actor.navigation.pending_arrival_clear
            || actor.movement_target.is_some()
            || super::track_head::active_track_family(actor).is_some()
        {
            return;
        }
        let nav = actor.navigation.nav_com.and_then(|target| {
            super::navcom::nav_target_coordinate(
                target,
                Some(id),
                &self.substrate.entities,
                self.resolved_terrain.as_ref(),
                rules.map(|rules| (rules, &self.interner)),
            )
            .ok()
            .map(|coord| (target, coord))
        });
        let info = self.resolve_move_info(id, rules);
        let timing = super::DestinationTiming::from_rules(self.session.binary_frame, rules);
        let terrain = self.resolved_terrain.as_ref();
        let Some(actor) = self.substrate.entities.get_mut(id) else {
            return;
        };
        actor.navigation.pending_arrival_clear = false;
        let speed = info.as_ref().map_or(SimFixed::lit("25"), |info| info.speed);
        let cell = |coord: DriveCoord| ((coord.x / 256) as u16, (coord.y / 256) as u16);
        let retained = match kind {
            LocomotorKind::Drive => actor.drive_locomotion.as_ref().and_then(|d| d.destination),
            _ => actor.ship_locomotion.as_ref().and_then(|s| s.destination),
        };
        let agrees =
            |destination: DriveCoord| nav.is_none_or(|(_, coord)| cell(coord) == cell(destination));
        if let Some(destination) = retained.filter(|&destination| agrees(destination)) {
            super::movement_commands::schedule_track_process(actor, cell(destination), speed);
        } else if let Some((target, coord)) = nav {
            let object = (!matches!(target, NavTargetRef::Cell { .. })).then_some((target, coord));
            super::movement_commands::prepare_track_destination(
                actor,
                cell(coord),
                object,
                speed,
                terrain,
                timing,
            );
        } else if let Some(NavTargetRef::Cell { rx, ry }) =
            actor.navigation.nav_queue.first().copied()
        {
            actor.navigation.nav_queue.remove(0);
            super::navcom::set_destination_internal_cell(actor, (rx, ry), terrain);
            timing.accept(actor);
            super::movement_commands::schedule_track_process(actor, (rx, ry), speed);
        } else {
            super::navcom::set_destination_internal_null(actor);
            return;
        }
        if let (Some(target), Some(info)) = (actor.movement_target.as_mut(), info) {
            target.accel_factor = info.accel_factor;
            target.decel_factor = info.decel_factor;
            target.slowdown_distance = info.slowdown_distance;
        }
    }

    /// EventClass MEGAMISSION: Assign_Target (0x4C7467), then the payload
    /// destination through the class setter +0x480 (0x4C747C). An ordered
    /// Attack carries none, so Walk takes Infantry 0x51AA40(NULL, 1) with its
    /// paid head kept and a Drive/Ship Unit takes 0x741970(NULL, 1): NavCom
    /// and +34 clear and the running track finishes at its head.
    pub(crate) fn finish_ordered_attack_destination(&mut self, id: u64, rules: Option<&RuleSet>) {
        if self.substrate.entities.get(id).is_some_and(track_unit) {
            self.set_unit_null_destination(id, rules);
        } else {
            self.finish_ordered_walk_attack(id, rules);
        }
    }

    /// The class setter vt+0x480 with a NULL destination from outside the
    /// locomotor: Foot::Override_Mission (0x4D8F6D, the ReceiveDamage
    /// retaliation) and Restore_Mission (0x4D8F99), the capture reset
    /// (0x70F859), the owner change (0x7014E9, 0x70182F), the parasite
    /// release (0x62A78A, 0x62A3ED, 0x62AAB9) and the Temporal freeze. A
    /// Drive/Ship Unit takes Unit 0x741970(NULL, 1), whose locomotor Stop
    /// nulls +34, so a track end cannot resume the old order; any other
    /// receiver keeps the represented NavCom write set.
    pub(crate) fn assign_null_destination(&mut self, id: u64, rules: Option<&RuleSet>) {
        if self.substrate.entities.get(id).is_some_and(track_unit) {
            self.set_unit_null_destination(id, rules);
        } else if let Some(actor) = self.substrate.entities.get_mut(id) {
            crate::sim::mission::concrete_effects::represented_assign_destination_mode_one(
                actor, None,
            );
        }
    }

    /// Unit 0x741970(NULL, 1) for a Drive/Ship receiver, as Find_Path's
    /// failure continuation (0x4D413A) and the Process continuations call it.
    /// - 0x741A80..0x741A90: without a NavCom (and without the +1F8 byte,
    ///   which VERA does not represent) it returns before any write.
    /// - 0x742D46..0x742E1F: while radio slot 0 holds a WeaponsFactory
    ///   building (BuildingType+16BD, ReadINI 0x460A72) and the current
    ///   mission is not Enter, it clears only NavQueue (+588) and the +5AC
    ///   vector (not represented) and returns: the exiting unit keeps NavCom.
    /// - 0x741E88: the live path word is cleared unless an Enter mission lacks
    ///   a radio contact; 0x7423BE clears NavQueue; 0x74314F enters Foot
    ///   0x4D94B0(NULL): NavComAux/NavCom, locomotor Stop (+0x48) and the
    ///   +640/+668 restart with +6B7 = 0.
    ///
    /// Residual (not represented): the BalloonHover arm (0x741983), the
    /// deploy-byte early return (0x741A9C), the Teleporter swap (0x7423CD),
    /// the +2B0 linked-object branch (0x742E3A) and the unpowered-locomotor
    /// PowerOn (0x742F48). Returns whether Foot 0x4D94B0 ran; the Rust
    /// scheduling adapter is then trimmed to the committed head.
    pub(crate) fn set_unit_null_destination(&mut self, id: u64, rules: Option<&RuleSet>) -> bool {
        let Some(actor) = self.substrate.entities.get(id) else {
            return false;
        };
        if actor.navigation.nav_com.is_none() {
            return false;
        }
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
        let timing = super::DestinationTiming::from_rules(self.session.binary_frame, rules);
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
        super::navcom::set_destination_internal_null(actor);
        timing.accept(actor);
        // The scheduling adapter keeps only the committed head step: the
        // locomotor Stop keeps the head, so the running track still finishes,
        // and its terminal then retires the adapter.
        super::retain_committed_movement(actor);
        true
    }
}
