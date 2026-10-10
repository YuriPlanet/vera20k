//! `HoverLocomotionClass` methods that move the Foot: Process (0x00514310)
//! with SpeedUpdate (0x00515ED0), the path/arrival continuation
//! (0x005164D0), ProcessMovement (0x00514F70) and the path trim
//! (0x005163B0), plus the Move_To (0x00514D90) and Stop_Moving (0x00516320)
//! state writes the Foot setter reaches.
//!
//! Hover shares the Foot's path words (+0x5E0), timers and latches with
//! Walk, Drive and Ship, and asks the one `Find_Path` (0x004D3920) and
//! Unit `Can_Enter_Cell` (0x0073F0A0) ports. It does not ride the movement
//! pass; `world::object_turn` calls [`Simulation::process_hover_locomotor`]
//! as the Foot's one Process.
//!
//! Residuals:
//! - Move_To's own `Path_And_Arrival(0)` and `ProcessMovement(1)`
//!   (0x00514E5A..0x00514F5C) run at the Foot's next Process, through
//!   SpeedUpdate's null-head arm (0x005162BA..0x00516309), which makes the
//!   same two calls after the same `Set_Speed(1.0)`: the Foot setter has no
//!   overlay registry for Can_Enter_Cell. Trigger: every Hover order.
//!   Effect: commands run before the object turns, so native's first
//!   Process after an order already has a head and turns and ramps; VERA's
//!   takes the null-head arm, so every order starts one frame later, its
//!   Can_Enter_Cell and cell claim run after the objects before it in the
//!   turn order have moved, and a Foot that already has a head keeps it
//!   until arrival instead of being re-admitted. Risk: order-to-motion
//!   timing and same-frame cell claims.
//! - Move_To's early return for a null coordinate (0x00514D90 entry) is not
//!   ported: every VERA caller passes a resolved coordinate, so it is dormant.
//! - The Foot's AbstractClass ID (Fetch_ID) phases the altitude bob
//!   (0x00513DF6); VERA keeps none, so the stable handle stands in. Effect:
//!   the ±2-lepton bob phase and period scale. Frequency: every Hover frame.
//! - The water wake (Rules+0x94 anim every 10 frames, 0x00514A21..0x00514AC3)
//!   comes from the shared wake pass (`Simulation::spawn_wakes_for_frame`,
//!   gated on the Hover's Is_Moving_Now), not from this Process tail. WAKE1
//!   draws no RNG; the residual is the anim's creation order (its AbstractClass
//!   ID) relative to the objects after this one.
//! - The vein tag (0x00486920) and DropIn over water below one level
//!   (vt+0xEC, 0x00514C0C) are not ported: the vein cell has no retail user;
//!   DropIn needs an unpowered Hover, and nothing in YR powers one off
//!   (Move_To's ion-storm test 0x0053A130 is constant false).
//! - An explicit map tube word (direction 8, 0x0051503B..0x005152CE) is
//!   answered as the invalid-tube arm: VERA's tube entry is bound to the Drive
//!   route adapter. Trigger: a Hover route through a map `[Tubes]` tube.
//!   Frequency: no retail YR map is known to author one.
//! - Crates (0x00481A00 at 0x005153E9) run through `Simulation::pickup_crate_at`,
//!   as in Drive; a host called without the OverlayType table answers as a
//!   cell without a crate.

use super::HoverRuntime;
use crate::map::resolved_terrain::{NativeCellQuery, ResolvedTerrainGrid};
use crate::rules::mission_data::MissionType;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::cell_kernel::{native_xy_distance, native_xyz_distance};
use crate::sim::components::{DriveCoord, NavTargetRef};
use crate::sim::game_entity::GameEntity;
use crate::sim::movement::foot_path::{FindPathResult, coord_cell};
use crate::sim::movement::ground_pose;
use crate::sim::movement::infantry_entry::InfantryEntryArgs;
use crate::sim::movement::movement_tick::FootPathRequest;
use crate::sim::world::{FrameAdvanceError, Simulation};
use crate::util::direction::DIRECTION_DELTAS;
use crate::util::direction_tables::facing16_from_delta;
use crate::util::fixed_math::{SIM_ONE, SIM_ZERO};
use crate::util::lepton::{BRIDGE_DECK_HEIGHT_LEPTONS, GROUND_LEVEL_HEIGHT_LEPTONS};
use crate::util::native_x87::distance_3d_leptons;

/// Foot+64C after a found route (0x00516BCD).
const FOUND_ROUTE_RETRIES: u32 = 10;

/// What one Hover Process did that its caller reports.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct HoverProcessOutcome {
    /// Per_Cell_Process(2) ran (0x005146CA or 0x00515A1C).
    pub(crate) per_cell_ran: bool,
    pub(crate) bridge_state_changed: bool,
}

fn runtime(entity: &GameEntity) -> Option<&HoverRuntime> {
    entity.locomotor.as_ref()?.hover_runtime()
}

fn runtime_mut(entity: &mut GameEntity) -> Option<&mut HoverRuntime> {
    entity.locomotor.as_mut()?.hover_runtime_mut()
}

fn null_coord(coord: Option<DriveCoord>) -> DriveCoord {
    coord.unwrap_or(DriveCoord { x: 0, y: 0, z: 0 })
}

fn path_word(entity: &GameEntity, index: usize) -> Option<u8> {
    entity
        .navigation
        .path_replay
        .remaining_directions()
        .get(index)
        .copied()
}

/// `Move_To` 0x00514D90 on a Hover Foot, as the Foot setter (0x004D94B0)
/// dispatches it: the destination, dropped to the ground (a deck on a
/// structural cell), then the speed request. Returns false when the Foot is
/// not on Hover. The Path_And_Arrival/ProcessMovement pair waits for the
/// next Process (module residual).
pub(crate) fn hover_move_to(
    entity: &mut GameEntity,
    coord: DriveCoord,
    terrain: Option<&ResolvedTerrainGrid>,
    frame: u32,
) -> bool {
    let paralyzed = entity.is_paralyzed(frame);
    let airborne = hover_is_powered(entity, terrain);
    let Some(hover) = runtime_mut(entity) else {
        return false;
    };
    //514D9C..514DBE: the coordinate is stored before the paralysis return.
    hover.destination = Some(coord);
    if paralyzed {
        return true;
    }
    //514DE5..514E26: ground height, plus the deck on a structural cell.
    let z = terrain.map_or(coord.z, |terrain| {
        let cells = NativeCellQuery::canonical(terrain);
        let ground = ground_pose::query_ground_height(&cells, coord).unwrap_or(coord.z);
        let structural = cells.flags(cells.lookup_world(coord.x, coord.y)) & 0x100 != 0;
        ground.wrapping_add(if structural {
            BRIDGE_DECK_HEIGHT_LEPTONS
        } else {
            0
        })
    });
    hover.destination = Some(DriveCoord { z, ..coord });
    if !airborne {
        return true;
    }
    // Its own Is_Moving_Now (`0x00514E66`): moving now only with a head; a
    // head leaves the request alone.
    let moving_now = hover.ready_state().is_moving_now();
    if moving_now && hover.head.is_some() {
        return true;
    }
    if moving_now {
        hover.speed_request = crate::util::native_x87::NativeF64Bits::ONE;
    }
    entity.foot_speed.set_speed_fraction(SIM_ONE);
    true
}

/// `Stop_Moving` 0x00516320 on a Hover Foot: a destination other than the
/// head is cleared with the Foot's path head (+0x5E0 = -1). Returns false
/// when the Foot is not on Hover.
pub(crate) fn hover_stop_moving(entity: &mut GameEntity) -> bool {
    let Some(hover) = runtime_mut(entity) else {
        return false;
    };
    if hover.stop_moving() {
        entity.navigation.path_replay.clear_live_head();
    }
    true
}

/// Hover's `Is_Powered` (0x00516C70): the locomotor's powered byte, or the
/// Foot above its surface (GetHeight 0x005F5F40 > 0).
fn hover_is_powered(entity: &GameEntity, terrain: Option<&ResolvedTerrainGrid>) -> bool {
    entity.locomotor.as_ref().is_some_and(|loco| loco.powered)
        || super::super::air_movement::current_fly_height(entity, terrain) > 0
}

impl Simulation {
    fn hover_error(&self, id: u64, cause: impl Into<String>) -> FrameAdvanceError {
        FrameAdvanceError {
            tick: self.session.tick,
            binary_frame: self.session.binary_frame,
            entity_id: id,
            cause: cause.into(),
        }
    }

    fn hover(&self, id: u64) -> Option<&HoverRuntime> {
        self.substrate.entities.get(id).and_then(runtime)
    }

    fn hover_mut(&mut self, id: u64) -> Option<&mut HoverRuntime> {
        self.substrate.entities.get_mut(id).and_then(runtime_mut)
    }

    fn hover_alive(&self, id: u64) -> bool {
        self.substrate
            .entities
            .get(id)
            .is_some_and(|e| e.lifecycle.object_alive)
    }

    fn hover_is_moving(&self, id: u64) -> bool {
        self.hover(id).is_some_and(HoverRuntime::is_moving)
    }

    fn hover_location(&self, id: u64) -> Option<DriveCoord> {
        self.substrate
            .entities
            .get(id)
            .map(|e| ground_pose::position_world_coord(&e.position))
    }

    /// `Set_Speed` (Foot vt+0x544, SetSpeedFraction 0x004D3710).
    fn hover_set_speed(&mut self, id: u64, full: bool) {
        if let Some(entity) = self.substrate.entities.get_mut(id) {
            entity
                .foot_speed
                .set_speed_fraction(if full { SIM_ONE } else { SIM_ZERO });
        }
    }

    /// The zeroed request/current pair and `Set_Speed(0)`.
    fn hover_halt(&mut self, id: u64) {
        if let Some(hover) = self.hover_mut(id) {
            hover.zero_speeds();
        }
        self.hover_set_speed(id, false);
    }

    /// `0x005152D7..0x0051531E` and its twin `0x0051541F..0x0051546D`: the
    /// Foot's path head (+0x5E0 = -1), the dummy head, `SetDestination(NULL,
    /// 1)`, the four zeroed speeds and `SetSpeedFraction(0.0)`.
    fn hover_drop_track(&mut self, id: u64, rules: &RuleSet) {
        if let Some(entity) = self.substrate.entities.get_mut(id) {
            entity.navigation.path_replay.clear_live_head();
            if let Some(hover) = runtime_mut(entity) {
                hover.head = None;
            }
        }
        self.set_unit_null_destination(id, Some(rules), None);
        self.hover_halt(id);
    }

    /// `if (head) { Unit vt+0xF4(head); head = null; }`.
    fn hover_release_head(&mut self, id: u64) {
        if let Some(head) = self.hover_mut(id).and_then(|hover| hover.head.take()) {
            self.track_raw_mark_at(id, head, false);
        }
    }

    /// The Foot's current speed (vt+0x538, GetCurrentSpeed 0x004DB1A0).
    fn hover_foot_speed(&self, id: u64, rules: &RuleSet) -> i32 {
        let Some(entity) = self.substrate.entities.get(id) else {
            return 0;
        };
        super::super::foot_speed::owner_current_speed(
            entity,
            self.object_type(entity.type_ref(), rules),
            rules.general.veteran_speed,
            &self.houses,
        )
    }

    /// FootClass::AI's Process call for a Hover Foot (ILocomotion +0x40,
    /// 0x00514310).
    pub(crate) fn process_hover_locomotor(
        &mut self,
        id: u64,
        rules: Option<&RuleSet>,
        registry: Option<&OverlayTypeRegistry>,
    ) -> Result<HoverProcessOutcome, FrameAdvanceError> {
        let rules = rules.ok_or_else(|| self.hover_error(id, "Hover Process requires rules"))?;
        let mut out = HoverProcessOutcome::default();
        if self.hover_is_moving(id) && !self.hover_move(id, rules, registry, &mut out)? {
            self.retire_hover_adapter(id);
            return Ok(out);
        }
        self.hover_process_tail(id, rules);
        self.retire_hover_adapter(id);
        Ok(out)
    }

    /// The `movement_target` a Hover order installs
    /// (`prepare_destination_execution`) tells its readers that the Foot
    /// moves; it drops once Is_Moving (0x00514C30) is false, with the
    /// waypoints the corridor lane dropped at arrival (#689 residual).
    fn retire_hover_adapter(&mut self, id: u64) {
        if let Some(entity) = self.substrate.entities.get_mut(id)
            && runtime(entity).is_some_and(|hover| !hover.is_moving())
        {
            entity.movement_target = None;
        }
    }

    /// Process 0x00514325..0x00514A1E. Returns false when Process returns
    /// before its tail.
    fn hover_move(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        out: &mut HoverProcessOutcome,
    ) -> Result<bool, FrameAdvanceError> {
        let frame = self.session.binary_frame;
        self.hover_speed_update(id, rules, registry, out)?;
        //514349..514358: a dead Foot returns before the tail.
        if !self.hover_alive(id) {
            return Ok(false);
        }
        let Some(start) = self.hover_location(id) else {
            return Ok(false);
        };
        let mut speed = self.hover(id).map_or(0, |hover| {
            hover.step_leptons(self.hover_foot_speed(id, rules))
        });
        let head = null_coord(self.hover(id).and_then(|hover| hover.head));
        let desired =
            facing16_from_delta(head.x.wrapping_sub(start.x), head.y.wrapping_sub(start.y));
        //5143DE..5144B9: a stopped Foot facing its head exactly restarts.
        let aligned = self
            .hover(id)
            .is_some_and(|hover| hover.facing.current(frame) == desired);
        if speed == 0 && aligned {
            let at_head_cell = self
                .hover(id)
                .and_then(|hover| hover.head)
                .is_none_or(|head| coord_cell(head) == coord_cell(start));
            if let Some(hover) = self.hover_mut(id) {
                hover.speed_request = crate::util::native_x87::NativeF64Bits::ONE;
            }
            self.hover_set_speed(id, true);
            self.hover_path_and_arrival(id, 0, rules, registry)?;
            if at_head_cell {
                self.hover_process_movement(id, true, rules, registry, out)?;
            }
            speed = self.hover(id).map_or(0, |hover| {
                hover.step_leptons(self.hover_foot_speed(id, rules))
            });
        }
        //5144BD..514513: the planar distance to the head, Location-based.
        let Some(location) = self.hover_location(id) else {
            return Ok(false);
        };
        let head = null_coord(self.hover(id).and_then(|hover| hover.head));
        let reach = native_xy_distance(
            location.x.wrapping_sub(head.x),
            location.y.wrapping_sub(head.y),
        );
        if speed >= reach {
            match self.hover_arrive(id, rules, registry, out)? {
                Arrival::Return => return Ok(false),
                Arrival::Tail => return Ok(true),
                Arrival::Step => {}
            }
        }
        if speed > 0 {
            self.hover_step(id, start.z, speed, rules, registry);
        }
        Ok(true)
    }

    /// The arrival arm 0x00514519..0x00514740.
    fn hover_arrive(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        out: &mut HoverProcessOutcome,
    ) -> Result<Arrival, FrameAdvanceError> {
        if let Some(entity) = self.substrate.entities.get_mut(id) {
            entity.foot_occupation_enabled = true;
            entity.navigation.path_runtime.path_blocked = false;
            if let Some(hover) = runtime_mut(entity) {
                hover.pushed = false;
            }
        }
        self.hover_path_and_arrival(id, 0, rules, registry)?;
        //51453A..514581: no path and no destination: the head drops (no F4).
        if let Some(entity) = self.substrate.entities.get_mut(id)
            && path_word(entity, 0).is_none()
            && let Some(hover) = runtime_mut(entity)
            && hover.destination.is_none()
        {
            hover.head = None;
        }
        //514584..5145FF: the cell of radio slot 0's vt+0x4C (with this
        //Foot as requester), else the null cell (0, 0).
        let Some(entity) = self.substrate.entities.get(id) else {
            return Ok(Arrival::Return);
        };
        let contact_cell = match entity.radio_contacts.slot(0) {
            Some(contact) => {
                let coord = super::super::navcom::nav_target_coordinate(
                    NavTargetRef::Object { id: contact },
                    Some(id),
                    &self.substrate.entities,
                    self.resolved_terrain.as_ref(),
                    Some((rules, &self.interner)),
                )
                .map_err(|cause| self.hover_error(id, cause))?;
                coord_cell(coord)
            }
            None => (0, 0),
        };
        let at_contact =
            coord_cell(ground_pose::position_world_coord(&entity.position)) == contact_cell;
        let has_destination = runtime(entity).is_some_and(|hover| hover.destination.is_some());
        if !has_destination && !at_contact {
            return Ok(Arrival::Step);
        }
        let code = self.hover_process_movement(id, true, rules, registry, out)?;
        if !self.track_survives(id) {
            return Ok(Arrival::Return);
        }
        let head_null = self.hover(id).is_some_and(|hover| hover.head.is_none());
        if code == 0 && head_null {
            //514698..5146EF: mark the Location, then Per_Cell_Process(2).
            if let Some(location) = self.hover_location(id) {
                self.track_raw_mark_at(id, location, true);
            }
            out.bridge_state_changed |= self.per_cell_process(
                id,
                super::super::PerCellReason::Arrival,
                Some(rules),
                registry,
            )?;
            out.per_cell_ran = true;
            if !self.track_survives(id) {
                return Ok(Arrival::Return);
            }
        }
        if code == 7 {
            //514705..514740: inside a tube the Foot returns; otherwise the
            //order ends and the Foot still takes this frame's step.
            if self
                .substrate
                .entities
                .get(id)
                .is_some_and(|e| e.low_bridge_tube_state.is_some())
            {
                return Ok(Arrival::Return);
            }
            self.set_unit_null_destination(id, Some(rules), None);
            self.hover_halt(id);
        } else if code != 0 {
            //514982..5149EA.
            self.hover_halt(id);
            self.hover_release_head(id);
            return Ok(Arrival::Tail);
        }
        Ok(Arrival::Step)
    }

    /// The XY step 0x00514746..0x00514973 along the steering facing.
    fn hover_step(
        &mut self,
        id: u64,
        z: i32,
        speed: i32,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) {
        let frame = self.session.binary_frame;
        let Some(location) = self.hover_location(id) else {
            return;
        };
        //514775..5147DF: the first moving frame releases the Location's
        //claim; +6B6 then keeps AddContent from marking until arrival.
        let release = self.substrate.entities.get(id).is_some_and(|entity| {
            entity.foot_occupation_enabled && runtime(entity).is_some_and(|h| h.head.is_some())
        });
        if release {
            self.track_raw_mark_at(id, location, false);
            if let Some(entity) = self.substrate.entities.get_mut(id) {
                entity.foot_occupation_enabled = false;
                entity.navigation.path_runtime.path_blocked = false;
            }
        }
        let Some(facing) = self.hover(id).map(|hover| hover.facing.current(frame)) else {
            return;
        };
        let [x, y] =
            crate::util::native_trig::facing_step_world_xy([location.x, location.y], facing, speed);
        let next = DriveCoord { x, y, z };
        if coord_cell(next) == coord_cell(location) {
            //5149F7..514A1E: SetCoords and SetZ with +74 cleared, so without
            //a Mark; VERA's SetCoords never marks.
            super::super::ground_pose::foot_set_location(
                &mut self.substrate.entities,
                id,
                next,
                Some(rules),
                &self.interner,
            );
            return;
        }
        //5148DF..514973: Mark(REMOVE), SetCoords, SetZ, the bridge byte from
        //the new cell and the height, Mark(PUT).
        self.foot_mark_remove(id, Some(rules), registry);
        super::super::ground_pose::foot_set_location(
            &mut self.substrate.entities,
            id,
            next,
            Some(rules),
            &self.interner,
        );
        let terrain = self.resolved_terrain.as_ref();
        if let (Some(terrain), Some(entity)) = (terrain, self.substrate.entities.get(id)) {
            let cells = NativeCellQuery::canonical(terrain);
            let structural = cells.flags(cells.lookup_world(x, y)) & 0x100 != 0;
            let height = super::super::air_movement::current_fly_height(entity, Some(terrain));
            let on_bridge =
                if !entity.on_bridge && structural && height >= BRIDGE_DECK_HEIGHT_LEPTONS {
                    true
                } else if entity.on_bridge && !structural {
                    false
                } else {
                    entity.on_bridge
                };
            if let Some(entity) = self.substrate.entities.get_mut(id) {
                entity.on_bridge = on_bridge;
            }
        }
        self.foot_mark_put(id, Some(rules), registry);
    }

    /// Process 0x00514A21..0x00514C12: the altitude controller and the
    /// Shove tail (see the module residuals for the wake, vein and DropIn).
    fn hover_process_tail(&mut self, id: u64, rules: &RuleSet) {
        let frame = self.session.binary_frame;
        self.hover_altitude(id, rules);
        let powered = self
            .substrate
            .entities
            .get(id)
            .is_some_and(|entity| hover_is_powered(entity, self.resolved_terrain.as_ref()));
        let Some(entity) = self.substrate.entities.get_mut(id) else {
            return;
        };
        let Some(hover) = runtime_mut(entity) else {
            return;
        };
        if !hover.shove_turning {
            return;
        }
        if !powered {
            hover.shove_turning = false;
            return;
        }
        //514AE5..514B47: PrimaryFacing.UpdateFacing(Current + steps << 8),
        //then one step back toward zero.
        let steps = hover.shove_steps;
        let paid = steps.signum();
        hover.shove_steps = steps - paid;
        if hover.shove_steps == 0 {
            hover.shove_turning = false;
        }
        let current = entity.body_facing.current(frame);
        entity
            .body_facing
            .snap(current.wrapping_add((steps << 8) as u16), frame);
    }

    /// 0x00513D20 on the Foot: GetHeight, the controller, SetHeight with the
    /// mark byte cleared.
    fn hover_altitude(&mut self, id: u64, rules: &RuleSet) {
        let frame = self.session.binary_frame as i32;
        let terrain = self.resolved_terrain.as_ref();
        let Some(entity) = self.substrate.entities.get_mut(id) else {
            return;
        };
        let height = super::super::air_movement::current_fly_height(entity, terrain);
        let location = ground_pose::position_world_coord(&entity.position);
        //513D45..513DDC: the ground (0x00578080) one direction step ahead
        //above the ground here.
        let climbing = terrain.is_some_and(|terrain| {
            let cells = NativeCellQuery::canonical(terrain);
            path_word(entity, 0).is_some_and(|direction| {
                let ahead = super::super::track_head::offset_head(location, direction);
                let ground = |at: DriveCoord| ground_pose::query_ground_height(&cells, at).ok();
                matches!((ground(ahead), ground(location)), (Some(next), Some(here)) if next > here)
            })
        });
        let powered = entity.locomotor.as_ref().is_some_and(|loco| loco.powered);
        let Some(hover) = runtime_mut(entity) else {
            return;
        };
        let visible = hover.altitude_step(height, climbing, id as i32, frame, powered, rules);
        //513E74..513E8C: SetHeight with +0x74 cleared, so unmarked.
        self.set_object_height_unmarked(id, visible);
    }

    /// SpeedUpdate 0x00515ED0.
    fn hover_speed_update(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        out: &mut HoverProcessOutcome,
    ) -> Result<(), FrameAdvanceError> {
        let frame = self.session.binary_frame;
        let terrain = self.resolved_terrain.as_ref();
        let Some(entity) = self.substrate.entities.get(id) else {
            return Ok(());
        };
        let Some(hover) = runtime(entity) else {
            return Ok(());
        };
        let Some(head) = hover.head else {
            //5162BA..516309: moving with no head asks for one.
            if hover.is_moving() {
                self.hover_set_speed(id, true);
                self.hover_path_and_arrival(id, 0, rules, registry)?;
                self.hover_process_movement(id, true, rules, registry, out)?;
            }
            return Ok(());
        };
        let foot = ground_pose::position_world_coord(&entity.position);
        let airborne = hover_is_powered(entity, terrain);
        let pushed = hover.pushed;
        let boost = matches!(
            (path_word(entity, 0), path_word(entity, 1)),
            (Some(first), Some(second)) if first == second
        );
        let look_ahead = match path_word(entity, 0) {
            Some(direction) if direction != crate::util::direction::TUBE_STEP_DIRECTION => {
                super::super::track_head::offset_head(head, direction)
            }
            _ => head,
        };
        let entity = self
            .substrate
            .entities
            .get_mut(id)
            .expect("same Hover Foot");
        let Some(hover) = runtime_mut(entity) else {
            return Ok(());
        };
        hover.choose_request(foot, head, airborne, frame);
        hover.ramp_speed(boost, rules);
        //5161E7..5162AD: a moving, unpushed Foot points its PrimaryFacing
        //one cell past the head.
        let request_positive =
            hover.speed_request.bits() != 0 && hover.speed_request.bits() >> 63 == 0;
        if request_positive && !pushed {
            let look = facing16_from_delta(
                look_ahead.x.wrapping_sub(foot.x),
                look_ahead.y.wrapping_sub(foot.y),
            );
            entity.body_facing.set(look, frame);
        }
        Ok(())
    }

    /// 0x005163B0: an object NavCom (Unit or Infantry) within 24 cells cuts
    /// the path there; else a TarCom the Foot can fire at drops the head.
    fn hover_trim_path(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        if !runtime(entity).is_some_and(HoverRuntime::is_moving) || path_word(entity, 0).is_none() {
            return;
        }
        let destination = null_coord(runtime(entity).and_then(|hover| hover.destination));
        if self.trim_path_for_object_navcom(id, destination) {
            return;
        }
        if self.foot_can_fire_at_target(id, rules, registry) == Some(true)
            && let Some(entity) = self.substrate.entities.get_mut(id)
        {
            entity.navigation.path_replay.clear_live_head();
        }
    }

    /// `Path_And_Arrival` 0x005164D0; `urgency` is Find_Path's third
    /// argument.
    fn hover_path_and_arrival(
        &mut self,
        id: u64,
        urgency: u8,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) -> Result<(), FrameAdvanceError> {
        let frame = self.session.binary_frame;
        self.hover_trim_path(id, rules, registry);
        let Some(entity) = self.substrate.entities.get(id) else {
            return Ok(());
        };
        let location = ground_pose::position_world_coord(&entity.position);
        let navigation = self
            .foot_navigation_coordinate(id)
            .map_err(|cause| self.hover_error(id, cause))?;
        let entity = self.substrate.entities.get(id).expect("same Hover Foot");
        let Some(destination) = runtime(entity).and_then(|hover| hover.destination) else {
            return Ok(());
        };
        let same_cell = coord_cell(location) == coord_cell(destination);
        let dz = navigation.z.wrapping_sub(destination.z).wrapping_abs();
        //5164E1..51657F: standing in the destination's cell and band stops.
        if same_cell && dz < 2 * GROUND_LEVEL_HEIGHT_LEPTONS {
            if let Some(entity) = self.substrate.entities.get_mut(id) {
                hover_stop_moving(entity);
            }
            self.set_unit_null_destination(id, Some(rules), None);
            return Ok(());
        }
        if path_word(entity, 0).is_some() {
            return Ok(());
        }
        if same_cell && dz <= 2 * GROUND_LEVEL_HEIGHT_LEPTONS {
            return Ok(());
        }
        if !entity
            .navigation
            .path_runtime
            .movement_timer
            .expired(frame as i32)
        {
            return Ok(());
        }
        let request = FootPathRequest::track(
            &self.substrate.entities,
            id,
            destination,
            urgency,
            self.playfield_bounds,
            Some(&self.type_handles),
            Some(rules),
        )
        .ok_or_else(|| self.hover_error(id, "retired Hover Find_Path requester"))?;
        let found = self
            .foot_find_path(&request, None, rules, registry)
            .map_err(|cause| self.hover_error(id, cause))?;
        //51668B..5166BA / 5168D0..516904: +640 = (Frame, PathDelay).
        if let Some(entity) = self.substrate.entities.get_mut(id) {
            entity
                .navigation
                .path_runtime
                .start_movement(frame, rules.general.path_delay_ticks());
        } else {
            return Ok(());
        }
        if found != FindPathResult::Failed {
            return self.hover_found_path(id, rules, registry);
        }
        self.hover_failed_path(id, rules, registry)
    }

    /// 0x005168D0..0x00516BCD after a found route.
    fn hover_found_path(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) -> Result<(), FrameAdvanceError> {
        let Some(entity) = self.substrate.entities.get(id) else {
            return Ok(());
        };
        if let Some(direction) = path_word(entity, 0) {
            let location = ground_pose::position_world_coord(&entity.position);
            let (dx, dy) = DIRECTION_DELTAS[usize::from(direction & 7)];
            let current = coord_cell(location);
            let cell = (i32::from(current.0) + dx, i32::from(current.1) + dy);
            if crate::sim::cell_rect::cell_is_in_playfield_height_aware(
                cell,
                self.playfield_bounds,
                self.resolved_terrain.as_ref(),
            ) && self.hover_can_enter(
                id,
                (cell.0 as i16, cell.1 as i16),
                direction,
                rules,
                registry,
            )? == 6
                && self.hover_ally_cell(id, cell, rules, registry)?
            {
                return Ok(());
            }
        }
        if let Some(entity) = self.substrate.entities.get_mut(id) {
            entity.navigation.path_runtime.retries_left = FOUND_ROUTE_RETRIES;
        }
        Ok(())
    }

    /// The found route's code-6 arm (0x005169BD..0x00516BC5): the nearest
    /// object of the cell's list (the deck above three levels), when an ally,
    /// either ends the move — close enough, no radio contact, in the stop
    /// band — or has the cell scattered. Returns true when the move ended.
    fn hover_ally_cell(
        &mut self,
        id: u64,
        cell: (i32, i32),
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) -> Result<bool, FrameAdvanceError> {
        let Some(terrain) = self.resolved_terrain.as_ref() else {
            return Ok(false);
        };
        let cells = NativeCellQuery::canonical(terrain);
        let Some(entity) = self.substrate.entities.get(id) else {
            return Ok(false);
        };
        let location = ground_pose::position_world_coord(&entity.position);
        let centre = DriveCoord {
            x: cell.0 * 256 + 128,
            y: cell.1 * 256 + 128,
            z: 0,
        };
        let ground = ground_pose::query_ground_height(&cells, centre)
            .map_err(|cause| self.hover_error(id, cause))?;
        let layer = if location.z > ground + 3 * GROUND_LEVEL_HEIGHT_LEPTONS {
            super::super::locomotor::MovementLayer::Bridge
        } else {
            super::super::locomotor::MovementLayer::Ground
        };
        let Some(nearest) = self.nearest_cell_object((cell.0 as u16, cell.1 as u16), layer, None)
        else {
            return Ok(false);
        };
        let allied = self.substrate.entities.get(nearest).is_some_and(|object| {
            crate::map::houses::is_allied_with(
                &self.house_alliances,
                self.interner.resolve(entity.owner()),
                self.interner.resolve(object.owner()),
            )
        });
        if !allied {
            return Ok(false);
        }
        let destination = null_coord(runtime(entity).and_then(|hover| hover.destination));
        let close = native_xyz_distance(
            location.x.wrapping_sub(destination.x),
            location.y.wrapping_sub(destination.y),
            location.z.wrapping_sub(destination.z),
        ) < rules.general.close_enough;
        if close && entity.radio_contacts.is_empty() && self.track_stop_band(location, destination)
        {
            if let Some(entity) = self.substrate.entities.get_mut(id) {
                hover_stop_moving(entity);
            }
            self.set_unit_null_destination(id, Some(rules), None);
            return Ok(true);
        }
        self.scatter_blocked_track_cell(id, (cell.0 as i16, cell.1 as i16), rules, registry)
            .map_err(|cause| self.hover_error(id, cause))?;
        Ok(false)
    }

    /// 0x0051668B..0x005168C3 after Find_Path refused.
    fn hover_failed_path(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) -> Result<(), FrameAdvanceError> {
        //5166BD..5166DD: the zone precheck on the live destination, which the
        //failed search's +500 receiver (Stop_Moving) may have nulled.
        let destination = null_coord(self.hover(id).and_then(|hover| hover.destination));
        if !self
            .foot_path_zone_precheck(id, destination, rules)
            .map_err(|cause| self.hover_error(id, cause))?
        {
            self.set_unit_null_destination(id, Some(rules), None);
            return Ok(());
        }
        let Some(entity) = self.substrate.entities.get(id) else {
            return Ok(());
        };
        let mission = entity.mission.effective().known();
        let location = ground_pose::position_world_coord(&entity.position);
        //5166E7..5167BD: not Enter, close enough, Move or AreaGuard.
        if mission != Some(MissionType::Enter)
            && native_xyz_distance(
                location.x.wrapping_sub(destination.x),
                location.y.wrapping_sub(destination.y),
                location.z.wrapping_sub(destination.z),
            ) < rules.general.close_enough
            && matches!(mission, Some(MissionType::Move | MissionType::AreaGuard))
        {
            self.set_unit_null_destination(id, Some(rules), None);
            if !self.hover_alive(id) {
                return Ok(());
            }
        } else if self
            .substrate
            .entities
            .get(id)
            .is_some_and(|e| e.navigation.path_runtime.retries_left > 0)
        {
            if let Some(entity) = self.substrate.entities.get_mut(id) {
                entity.navigation.path_runtime.retries_left -= 1;
            }
        } else {
            //5167D5..51681B: give up, with the retained scold sound.
            self.set_unit_null_destination(id, Some(rules), None);
            if !self.hover_alive(id) {
                return Ok(());
            }
            self.play_foot_path_scold(id, rules);
            if let Some(entity) = self.substrate.entities.get_mut(id) {
                entity.navigation.path_runtime.clear_scold_latch();
            }
        }
        //516822..5168C3: a stopped Foot drops a target it cannot fire at;
        //then the head and Stop_Moving.
        if !self.hover_is_moving(id) {
            self.drop_unfireable_target(id, rules, registry);
        }
        self.hover_release_head(id);
        if let Some(entity) = self.substrate.entities.get_mut(id) {
            hover_stop_moving(entity);
        }
        Ok(())
    }

    /// Unit `Can_Enter_Cell` (+0x1AC) for a Hover candidate, with the Techno
    /// height (0x005F5F00).
    fn hover_can_enter(
        &self,
        id: u64,
        cell: (i16, i16),
        direction: u8,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) -> Result<u8, FrameAdvanceError> {
        let terrain = self
            .resolved_terrain
            .as_ref()
            .ok_or_else(|| self.hover_error(id, "Hover Can_Enter_Cell requires map cells"))?;
        let entity = self
            .substrate
            .entities
            .get(id)
            .ok_or_else(|| self.hover_error(id, "retired Hover Foot"))?;
        let cells = NativeCellQuery::canonical(terrain);
        let height = ground_pose::query_object_cell_height(
            &cells,
            ground_pose::position_world_coord(&entity.position),
            entity.on_bridge,
        );
        self.foot_can_enter(
            id,
            terrain.native_cell_identity(cell),
            InfantryEntryArgs {
                direction: i32::from(direction),
                height,
                previous_cell: None,
            },
            rules,
            registry,
        )
        .map_err(|cause| self.hover_error(id, cause))
    }

    /// `ProcessMovement` 0x00514F70; `retry` is its argument. Returns its
    /// code: 0 accepted or no work, 7 stopped, else the refusal code.
    fn hover_process_movement(
        &mut self,
        id: u64,
        retry: bool,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        out: &mut HoverProcessOutcome,
    ) -> Result<u8, FrameAdvanceError> {
        let frame = self.session.binary_frame;
        //514F83..514FCA: the old head's claim goes first.
        self.hover_release_head(id);
        let Some(entity) = self.substrate.entities.get(id) else {
            return Ok(7);
        };
        if entity.is_paralyzed(frame) {
            return Ok(7);
        }
        let Some(hover) = runtime(entity) else {
            return Ok(0);
        };
        if !hover.is_moving() || hover.destination.is_none() {
            return Ok(0);
        }
        let Some(direction) = path_word(entity, 0) else {
            //515DEE: no path word asks the continuation.
            self.hover_path_and_arrival(id, 0, rules, registry)?;
            return Ok(0);
        };
        if direction == crate::util::direction::TUBE_STEP_DIRECTION {
            //5152D1..51532E: the invalid-tube arm (module residual): the
            //drop, then one more null destination.
            self.hover_drop_track(id, rules);
            self.set_unit_null_destination(id, Some(rules), None);
            return Ok(7);
        }
        let location = ground_pose::position_world_coord(&entity.position);
        let current = coord_cell(location);
        let (dx, dy) = DIRECTION_DELTAS[usize::from(direction & 7)];
        let cell = (
            current.0.wrapping_add(dx as i16),
            current.1.wrapping_add(dy as i16),
        );
        let terrain = self
            .resolved_terrain
            .as_ref()
            .ok_or_else(|| self.hover_error(id, "Hover ProcessMovement requires map cells"))?;
        let cells = NativeCellQuery::canonical(terrain);
        //51533C..5153D5: the head is the candidate's centre on its ground, or
        //its deck when the Foot rides three levels above that ground.
        let mut head = DriveCoord {
            x: i32::from(cell.0) * 256 + 128,
            y: i32::from(cell.1) * 256 + 128,
            z: 0,
        };
        let ground = ground_pose::query_ground_height(&cells, head)
            .map_err(|cause| self.hover_error(id, cause))?;
        head.z = if ground + 3 * GROUND_LEVEL_HEIGHT_LEPTONS <= location.z {
            ground + BRIDGE_DECK_HEIGHT_LEPTONS
        } else {
            ground
        };
        let probe = super::super::track_head::offset_head(location, direction);
        let probe_structural = cells.flags(cells.lookup_world(probe.x, probe.y)) & 0x100 != 0;
        if let Some(hover) = self.hover_mut(id) {
            hover.head = Some(head);
        }
        //5153D8..5153EE: the crate question on the candidate cell.
        if !self.pickup_crate_at(id, cell, Some(rules), registry) {
            //5153F6..515478: a false answer (a placed free vehicle): an owner
            //in limbo rejoins the ordinary checks; a dead or falling one
            //returns; a live one drops its path head, head and speeds and
            //returns.
            let in_limbo = self
                .substrate
                .entities
                .get(id)
                .is_some_and(|entity| entity.lifecycle.in_limbo);
            if !in_limbo {
                if self.track_survives(id) {
                    self.hover_drop_track(id, rules);
                }
                return Ok(7);
            }
        }
        //51547B..5154A2: a dead, limbo or falling Foot returns.
        if !self.track_survives(id) {
            return Ok(7);
        }
        //5154A8..515513: the candidate cell's bridge bit against OnBridge.
        let entity = self
            .substrate
            .entities
            .get(id)
            .expect("surviving Hover Foot");
        if probe_structural != entity.on_bridge {
            self.latch_foot_68b(id);
        }
        let code = self.hover_can_enter(id, cell, direction, rules, registry)?;
        match code {
            0 => {
                //515A1C: Per_Cell_Process(2), then the claim and the shift.
                out.bridge_state_changed |= self.per_cell_process(
                    id,
                    super::super::PerCellReason::Arrival,
                    Some(rules),
                    registry,
                )?;
                out.per_cell_ran = true;
                if !self.track_survives(id) {
                    return Ok(7);
                }
                let in_playfield = self
                    .substrate
                    .entities
                    .get(id)
                    .is_some_and(|entity| entity.in_playfield);
                if in_playfield && let Some(head) = self.hover(id).and_then(|hover| hover.head) {
                    self.track_raw_mark_at(id, head, true);
                }
                if let Some(entity) = self.substrate.entities.get_mut(id) {
                    let queue = &mut entity.navigation.path_replay;
                    super::super::path_markers::consume_path_replay(queue, 1);
                    queue.reference_cell = Some(coord_cell(head));
                }
                Ok(0)
            }
            1 | 7 => {
                self.uncloak_contacts_at_cell(cell, rules)
                    .map_err(|cause| self.hover_error(id, cause))?;
                let mut code = code;
                if retry {
                    self.hover_halt(id);
                    self.hover_release_head(id);
                    self.hover_restart_path(id);
                    self.hover_path_and_arrival(id, 0, rules, registry)?;
                    code = self.hover_process_movement(id, false, rules, registry, out)?;
                }
                if code != 0 {
                    if let Some(hover) = self.hover_mut(id) {
                        hover.head = None;
                    }
                    self.hover_halt(id);
                }
                Ok(code)
            }
            2 => {
                if let Some(hover) = self.hover_mut(id) {
                    hover.head = None;
                }
                self.hover_halt(id);
                let entity = self
                    .substrate
                    .entities
                    .get_mut(id)
                    .expect("same Hover Foot");
                let runtime = &mut entity.navigation.path_runtime;
                if !runtime.path_blocked {
                    runtime.path_blocked = true;
                    runtime.start_blocked(frame, rules.general.blockage_path_delay_ticks);
                }
                if runtime.movement_timer.expired(frame as i32) {
                    entity.navigation.path_replay.clear_live_head();
                }
                let runtime = &entity.navigation.path_runtime;
                let urgency = if runtime.path_blocked && runtime.blocked_timer.expired(frame as i32)
                {
                    2
                } else {
                    1
                };
                self.hover_path_and_arrival(id, urgency, rules, registry)?;
                Ok(2)
            }
            3 => {
                if let Some(hover) = self.hover_mut(id) {
                    hover.head = None;
                }
                let _ = crate::sim::gate_runtime::request_gate_open_for_cell(
                    self,
                    (cell.0 as u16, cell.1 as u16),
                    id,
                    rules,
                );
                self.hover_halt(id);
                Ok(3)
            }
            4 | 5 => {
                if let Some(hover) = self.hover_mut(id) {
                    hover.head = None;
                }
                if retry {
                    self.hover_halt(id);
                    self.hover_restart_path(id);
                    self.hover_path_and_arrival(id, 0, rules, registry)?;
                    return self.hover_process_movement(id, false, rules, registry, out);
                }
                self.override_movement_blocker_at(id, cell, rules, registry);
                Ok(code)
            }
            6 => {
                if retry {
                    if let Some(hover) = self.hover_mut(id) {
                        hover.head = None;
                    }
                    self.hover_halt(id);
                    self.hover_restart_path(id);
                    self.hover_path_and_arrival(id, 0, rules, registry)?;
                    return self.hover_process_movement(id, false, rules, registry, out);
                }
                //51581D..5158F4: close enough and in the stop band ends it.
                let entity = self.substrate.entities.get(id).expect("same Hover Foot");
                let destination = null_coord(runtime(entity).and_then(|hover| hover.destination));
                let close = distance_3d_leptons(
                    [location.x, location.y, location.z],
                    [destination.x, destination.y, destination.z],
                ) < rules.general.close_enough;
                if close && self.track_stop_band(location, destination) {
                    if let Some(entity) = self.substrate.entities.get_mut(id) {
                        hover_stop_moving(entity);
                    }
                    self.set_unit_null_destination(id, Some(rules), None);
                    return Ok(7);
                }
                //515902..5159E1: scatter the head cell; the head drops.
                self.scatter_blocked_track_cell(id, cell, rules, registry)
                    .map_err(|cause| self.hover_error(id, cause))?;
                if let Some(hover) = self.hover_mut(id) {
                    hover.head = None;
                }
                self.hover_halt(id);
                Ok(6)
            }
            other => Err(self.hover_error(id, format!("Can_Enter_Cell code {other}"))),
        }
    }

    /// The retry preamble: +640 = (Frame, 0) and the path head dropped.
    fn hover_restart_path(&mut self, id: u64) {
        let frame = self.session.binary_frame;
        if let Some(entity) = self.substrate.entities.get_mut(id) {
            entity.navigation.path_runtime.start_movement(frame, 0);
            entity.navigation.path_replay.clear_live_head();
        }
    }
}

enum Arrival {
    /// Process returns before its tail.
    Return,
    /// Skip the step, run the tail.
    Tail,
    /// Take the step, then the tail.
    Step,
}
