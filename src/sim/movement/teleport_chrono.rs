//! The Teleport's Chronosphere states: `TeleportLocomotionClass::Process @
//! 0x007192F0` once a Chrono Warp (`superweapon::chronosphere`, case 4)
//! latched its owner (Techno `+0x27C`, `0x00719351`) or its state (`+0x38`)
//! is past 0 (`0x0071935F`), with TimerCheck (`0x00719BF0`), Update_Position
//! (`0x00718260`) and PostWarpValidation (`0x007187A0`).
//!
//! One call of the Process runs one state:
//! - 0: BeingWarpedOut (`+0x270`) and a 60-frame timer; -> 1
//!   (`0x007197D0..0x007197F4`).
//! - 1 and 6: [TimerCheck](Simulation::teleport_timer_check); its expiry
//!   advances the state.
//! - 2: `[General] WarpOut=` at the Location, Mark(UP), ChronoOut, WarpingIn
//!   (`+0x271`) up, the latch, BeingWarpedOut and OnBridge (`+0x8C`) down,
//!   then [Update_Position](Simulation::chrono_update_position) on the
//!   destination: -> 4 when it lands, -> 3 when the cell is blocked
//!   (`0x00719827..0x00719936`).
//! - 3: Update_Position again, on the nearby cell the blocked search chose
//!   (-> 4 when it lands), and `+0x284` takes `[General] ChronoDelay=`
//!   (`0x00719948..0x00719983`).
//! - 4: Update_Position moves the occupation and the Marked coordinate, then
//!   SetLocation there, SetHeight(0) and Mark(DOWN); -> 5
//!   (`0x00719998..0x007199E9`).
//! - 5: SetLocation, SetHeight(0) and Mark(DOWN) again, ChronoIn, the
//!   playfield byte (`+0x3D5`), [PostWarpValidation](
//!   Simulation::chrono_post_warp_validation); a survivor then takes
//!   PerCell(2), the Teleport's Stop_Moving, the house (`+0x42C`) and archive
//!   target cleared, a NULL destination, the timer at `+0x284` and WarpOut
//!   again; -> 6 (`0x00719A01..0x00719B85`).
//! - 7: WarpingIn down, the archive target cleared, a NULL destination,
//!   Is_Moving down; -> 0 (`0x00719BB4..0x00719BDF`), after which the next
//!   IPiggyback END ends the piggyback (`Simulation::try_end_piggyback`):
//!   an idle entry's (`0x004D831A`) or the Foot AI tail's.
//!
//! FootClass::AI's Process call runs it, and so does each class AI's prologue
//! while the object is warping in, or warped out with the latch
//! (`Simulation::temporal_ai_prologue`): state 1 freezes the object, and the
//! warp itself (states 2 to 4) takes both calls of one frame.
//! Reinforcement65EC30 instead starts with WarpingIn and pending state3
//! (+0x280): one Process transfers it, state3 rewrites the delay, pending
//! skips state5's PostWarpValidation, and state7 clears it. Executed controls:
//! tools/spatial_oracle/trigger_chrono_reinforcement.json, two supplied-visit
//! histories including TimerCheck's NULL-target draw. These controls do not
//! certify complete class-AI cadence or caller-side End_Piggyback.
//!
//! Techno `+0x284` is written on a blocked landing (state 3), so an
//! unblocked landing warps in after the object's previous blocked landing's
//! delay, 0 for the first.
//!
//! Scenario draws: TimerCheck's scan when no target is held (`RandomRanged(0,
//! 2)` in Retaliate_And_Scan, the ShortenPassiveScanTimer draw) and the
//! idle-mode selection it may reach; the kills and sinks draw through their
//! receivers. Timer writes: the Teleport timer (states 0 and 5), the passive
//! scan timer (TimerCheck). Detach calls: none of its own.
//!
//! Evidence: instruction reading of the addresses cited here.
//!
//! RESIDUALS:
//! - A destination given while warping in (states 2 to 7, the latch down)
//!   runs the Unit setter's Teleporter arm, whose Drive suspends the warp's
//!   Teleport (`locomotor_owner::begin_drive_for_teleporter`), refuses the
//!   destination while its owner warps (`0x004AFD40`) and ends at the Foot
//!   AI tail. The NavCom it published stays, so state 5's NULL destination,
//!   which the prologue runs, installs another Drive: FootClass::AI's
//!   Process runs that Drive, and state 6's TimerCheck with its scan draw
//!   waits for the next prologue. That frame and draw order is read, not
//!   executed. Natively the prologue also runs a Drive active at its call
//!   (`0x007362A7..0x007362F5`); VERA runs none. Trigger: a destination for
//!   a Chrono Miner in the warp's last frames. Effect: TimerCheck's draw
//!   moves a frame and the warp's END up to one; the Drive goes nowhere.
//! - A landed Aircraft that sinks keeps `+0x3CD` with nothing to end it:
//!   VERA ports only the Unit's sinking (Unit AI `0x007364A1`); the Aircraft
//!   AI's handling is not established. Triggers: an airfield's Harriers
//!   chronoshifted onto water, or a Magnetron dropping a lifted Aircraft over
//!   water it is refused (`jumpjet_flight::state5_crash`'s sink).

use crate::map::entities::EntityCategory;
use crate::map::resolved_terrain::NativeCellQuery;
use crate::rules::locomotor_type::{MovementZone, SpeedType};
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::rules::terrain_rules::LandType;
use crate::sim::components::DriveCoord;
use crate::sim::find_nearby_cell::{NearbyArgs, NearbyFootprint, PassabilityArgs};
use crate::sim::movement::infantry_entry::{EntryQueryMode, InfantryEntryArgs};
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::movement::teleport_movement::{ChronoWarp, WarpSound};
use crate::sim::occupancy::CellObjectMember;
use crate::sim::world::{FrameAdvanceError, FrameEffects, ObjectAiCtx, Simulation};

/// State 0's timer (`0x007197DF`, `MOV ECX,0x3C`).
const CHRONO_WARP_OUT_FRAMES: i32 = 0x3C;
/// CellClass `+0x140`: a bridge over the cell, and the bit Update_Position
/// requires with it (`0x00718452..0x0071846E`).
const CELL_FLAG_BRIDGE: u32 = 0x100;
const CELL_FLAG_BRIDGE_DECK: u32 = 0x200;

impl Simulation {
    /// Foot4DF7F0 / Infantry522FE0, virtual508. Source: unchanged active-YR
    /// bodies, Steam gamemd (tools.native_inspect). Reuses the single
    /// Teleport/piggyback warp owner; no action-specific position/state copy.
    /// No own RNG/timer/detach: class destination and Walk sub-cell consumers
    /// retain theirs, and subsequent Teleport Process owns warp timers.
    pub(crate) fn foot_chrono_warp_to(
        &mut self,
        id: u64,
        mut coord: DriveCoord,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> Result<bool, String> {
        let actor = self
            .substrate
            .entities
            .get(id)
            .ok_or("ChronoWarpTo missing actor")?;
        if super::air_movement::is_high_flying(
            actor,
            self.resolved_terrain.as_ref(),
            Some((rules, &self.interner)),
        ) || crate::sim::superweapon::invulnerability::is_invulnerable(
            actor.invulnerability.as_ref(),
            self.session.binary_frame(),
        ) || actor.chrono_warp_latch()
            || actor.is_warped_out()
            || actor.is_warping_in()
        {
            return Ok(false);
        }
        let category = actor.category();
        let infantry = category == EntityCategory::Infantry;
        if actor.locomotor.is_none() {
            return Err("ChronoWarpTo actor has no locomotor".into());
        }
        let mut cell = super::coord_cell(coord);
        //Infantry prepares its old Walk head before creating the Teleport;
        //Foot adjusts the bridge first, then prepares Unit Force_Track.
        if infantry {
            self.locomotor_mark_all_occupation_bits(id, false);
            Self::object_raw_receiver_at(
                &mut self.substrate,
                &self.path_grid,
                &self.resolved_terrain,
                id,
                coord,
                false,
            );
            let Some(actor) = self.substrate.entities.get_mut(id) else {
                return Ok(false);
            };
            actor.foot_occupation_enabled.resume();
            self.assign_destination_represented(
                id,
                Some(crate::sim::components::NavTargetRef::cell(
                    cell.0 as u16,
                    cell.1 as u16,
                )),
                Some(rules),
                registry,
                effects,
            )
            .map_err(|error| error.to_string())?;
            self.foot_force_immediate_destination(id, Some(coord), Some(rules), registry, effects)?;
        }
        let terrain = self
            .resolved_terrain
            .as_ref()
            .ok_or("ChronoWarpTo needs map cells")?;
        let cells = NativeCellQuery::canonical(terrain);
        if cells.flags(cells.lookup_world(coord.x, coord.y)) & CELL_FLAG_BRIDGE != 0 {
            coord.z = coord
                .z
                .wrapping_add(crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS);
        }
        if category == EntityCategory::Unit {
            self.locomotor_mark_all_occupation_bits(id, false);
            self.force_track(id, -1, coord, Some(rules), registry, effects);
            Self::object_raw_receiver_at(
                &mut self.substrate,
                &self.path_grid,
                &self.resolved_terrain,
                id,
                coord,
                false,
            );
            let Some(actor) = self.substrate.entities.get_mut(id) else {
                return Ok(false);
            };
            actor.foot_occupation_enabled.resume();
            self.assign_destination_represented(
                id,
                Some(crate::sim::components::NavTargetRef::cell(
                    cell.0 as u16,
                    cell.1 as u16,
                )),
                Some(rules),
                registry,
                effects,
            )
            .map_err(|error| error.to_string())?;
            self.assign_destination_represented(id, None, Some(rules), registry, effects)
                .map_err(|error| error.to_string())?;
        }
        let frame = self.session.binary_frame();
        let Some(loco) = self
            .substrate
            .entities
            .get_mut(id)
            .and_then(|actor| actor.locomotor.as_mut())
        else {
            return Ok(false);
        };
        loco.begin_piggyback(crate::rules::locomotor_type::LocomotorKind::Teleport, frame);
        loco.teleport_runtime_mut()
            .ok_or("ChronoWarpTo Teleport construction failed")?
            .arm_chrono(ChronoWarp::new(coord, frame));
        cell = super::coord_cell(coord);
        self.assign_destination_represented(
            id,
            Some(crate::sim::components::NavTargetRef::cell(
                cell.0 as u16,
                cell.1 as u16,
            )),
            Some(rules),
            registry,
            effects,
        )
        .map_err(|error| error.to_string())?;
        if infantry {
            self.locomotor_mark_all_occupation_bits(id, false);
        }
        Ok(true)
    }
}

fn foot(category: EntityCategory) -> bool {
    matches!(
        category,
        EntityCategory::Unit | EntityCategory::Infantry | EntityCategory::Aircraft
    )
}

impl Simulation {
    /// Object `+0x90`.
    fn chrono_owner_alive(&self, id: u64) -> bool {
        self.substrate
            .entities
            .get(id)
            .is_some_and(|entity| !entity.death.is_dying() && entity.lifecycle.object_alive())
    }

    /// The Teleport Process of an owner whose Teleport holds a Chronosphere
    /// warp: the state it is in (module docs). Answers whether state 5's
    /// `Per_Cell_Process(2)` changed a bridge's state. Each state is
    /// `TeleportLocomotionClass::Process @ 0x007192F0`'s own
    /// (`0x007197CC..0x00719BE2`), which stores the warp's state (`+0x38`),
    /// its timer and the Techno bytes.
    pub(crate) fn process_chrono_warp(
        &mut self,
        id: u64,
        rules: &RuleSet,
        ctx: ObjectAiCtx<'_>,
    ) -> Result<bool, FrameAdvanceError> {
        let registry = ctx.overlay_registry;
        let mut bridge_state_changed = false;
        let Some(warp) = self
            .substrate
            .entities
            .get(id)
            .and_then(|entity| entity.chrono_warp())
            .cloned()
        else {
            return Ok(bridge_state_changed);
        };
        let frame = self.session.binary_frame();
        // Original719304..719342 precedes both the ordinary movement and
        // Chronosphere arms. Reinforcement's pending3 transfers on one visit;
        // the next visit runs state3. WarpingIn alone instead runs TimerCheck.
        if warp.state() == 0 && warp.warping_in() && warp.pending_state() == 0 {
            self.teleport_timer_check(id, Some(rules), ctx);
            return Ok(false);
        }
        if warp.state() == 0 && warp.pending_state() != 0 {
            if let Some(active) = self.substrate.chrono_warp_mut(id) {
                active.set_state(warp.pending_state());
            }
            return Ok(false);
        }
        match warp.state() {
            0 => {
                if let Some(warp) = self.substrate.chrono_warp_mut(id) {
                    warp.set_bytes(true, true, false);
                    warp.start_timer(frame, CHRONO_WARP_OUT_FRAMES);
                    warp.set_state(1);
                }
            }
            1 | 6 => {
                self.teleport_timer_check(id, Some(rules), ctx);
            }
            2 => {
                self.teleport_warp_out(id, rules);
                self.foot_mark_remove(id, Some(rules), registry, ctx.effects);
                Self::teleport_warp_sound(
                    &mut self.interner,
                    &self.substrate,
                    &mut self.sound_events,
                    id,
                    WarpSound::Out,
                    rules,
                );
                if let Some(entity) = self.substrate.entities.get_mut(id) {
                    // The warp-out's OnBridge clear (`0x007198FA`).
                    entity.on_bridge.store(false);
                    if let Some(locomotor) = entity.locomotor.as_mut() {
                        locomotor.store_ground_layer(false);
                    }
                }
                if let Some(warp) = self.substrate.chrono_warp_mut(id) {
                    warp.set_bytes(false, false, true);
                }
                let landed = self.chrono_update_position(
                    id,
                    warp.destination(),
                    false,
                    rules,
                    registry,
                    ctx.effects,
                );
                if let Some(warp) = self.substrate.chrono_warp_mut(id) {
                    warp.set_state(if landed { 4 } else { 3 });
                }
            }
            3 => {
                let landed = self.chrono_update_position(
                    id,
                    warp.destination(),
                    false,
                    rules,
                    registry,
                    ctx.effects,
                );
                if landed && let Some(warp) = self.substrate.chrono_warp_mut(id) {
                    warp.set_state(4);
                }
                if let Some(entity) = self.substrate.entities.get_mut(id) {
                    entity.chrono_warp_delay.store(rules.general.chrono_delay);
                }
            }
            4 => {
                self.chrono_update_position(
                    id,
                    warp.destination(),
                    true,
                    rules,
                    registry,
                    ctx.effects,
                );
                self.chrono_settle(id, rules, registry, ctx.effects);
                if let Some(warp) = self.substrate.chrono_warp_mut(id) {
                    warp.set_state(5);
                }
            }
            5 => {
                let marked = self.chrono_settle(id, rules, registry, ctx.effects);
                Self::teleport_warp_sound(
                    &mut self.interner,
                    &self.substrate,
                    &mut self.sound_events,
                    id,
                    WarpSound::In,
                    rules,
                );
                self.chrono_playfield_check(id);
                //719AA3..719AC9: a newly reinforced member skips the
                //Chronosphere's terrain kill/sink/transform validation.
                if warp.pending_state() == 0 {
                    self.chrono_post_warp_validation(id, marked, rules, registry, ctx.effects);
                }
                if !self.chrono_owner_alive(id) {
                    return Ok(bridge_state_changed);
                }
                bridge_state_changed |= self.per_cell_process(
                    id,
                    super::PerCellReason::Arrival,
                    Some(rules),
                    registry,
                    ctx.effects,
                )?;
                if let Some(entity) = self.substrate.entities.get_mut(id) {
                    super::teleport_movement::teleport_stop_moving(entity);
                    // `0x00719AEE`/`0x00719AF7`: the drop and warp credits go.
                    entity.drop_credit.forget();
                    entity.set_archive_target(None);
                }
                self.assign_null_destination(id, Some(rules), registry, ctx.effects);
                let delay = self
                    .substrate
                    .entities
                    .get(id)
                    .map_or(0, |entity| entity.chrono_warp_delay());
                if let Some(warp) = self.substrate.chrono_warp_mut(id) {
                    warp.start_timer(frame, delay);
                }
                self.teleport_warp_out(id, rules);
                if let Some(warp) = self.substrate.chrono_warp_mut(id) {
                    warp.set_state(6);
                }
            }
            7 => {
                if let Some(warp) = self.substrate.chrono_warp_mut(id) {
                    warp.set_warping_in(false);
                }
                if let Some(entity) = self.substrate.entities.get_mut(id) {
                    entity.set_archive_target(None);
                }
                self.assign_null_destination(id, Some(rules), registry, ctx.effects);
                if let Some(runtime) = self
                    .substrate
                    .entities
                    .get_mut(id)
                    .and_then(|entity| entity.locomotor.as_mut())
                    .and_then(|locomotor| locomotor.warp_teleport_mut())
                {
                    runtime.end_chrono();
                }
            }
            _ => {}
        }
        Ok(bridge_state_changed)
    }

    /// States 4 and 5's `SetLocation(Marked)` (FootClass vt+0x1B4),
    /// `SetHeight(0)` (vt+0x1CC) and `Mark(DOWN)` (vt+0x124(1)). Answers the
    /// Marked coordinate (Teleport `+0x28`).
    fn chrono_settle(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> DriveCoord {
        let marked = self
            .substrate
            .entities
            .get(id)
            .and_then(|entity| entity.locomotor.as_ref())
            .and_then(|locomotor| locomotor.teleport_runtime())
            .and_then(|runtime| runtime.resolved_destination())
            .unwrap_or(DriveCoord { x: 0, y: 0, z: 0 });
        self.foot_set_location_marked(id, marked, Some(rules), registry, effects);
        self.set_object_height(id, 0, Some(rules), registry, effects);
        self.foot_mark_put(id, Some(rules), registry, effects);
        marked
    }

    /// State 5's playfield test (`0x00719A75..0x00719A99`): the cell of the
    /// Location (vt+0x1B8) outside `MapClass::IsCellInPlayfield(cell, 1)`
    /// clears `+0x3D5`; inside, the byte stays as it was.
    fn chrono_playfield_check(&mut self, id: u64) {
        let Some(cell) = self.substrate.entities.get(id).map(|entity| {
            (
                i32::from(entity.position.rx()),
                i32::from(entity.position.ry()),
            )
        }) else {
            return;
        };
        if !crate::sim::cell_rect::cell_is_in_playfield_height_aware(
            cell,
            self.playfield_bounds(),
            self.resolved_terrain.as_ref(),
        ) && let Some(entity) = self.substrate.entities.get_mut(id)
        {
            entity.in_playfield.leave();
        }
    }

    /// `TeleportLocomotionClass::Update_Position @ 0x00718260` on `coord`
    /// (the owner's `+0x288`).
    ///
    /// Placing (`0x0071865B..0x00718792`): the owner's raw occupation leaves
    /// the Marked coordinate (Teleport `+0x28`, the Location while it is the
    /// null coordinate; vt+0xF4), Marked becomes `+0x288` with the ground
    /// height there (`0x00578080`), raised by the bridge height with OnBridge
    /// set when the cell has a bridge and the owner was not on one, otherwise
    /// OnBridge cleared; then two raw PUTs there (vt+0xF0). Answers true.
    ///
    /// Testing (`0x00718275..0x00718658`), on the coordinate's cell list (the
    /// bridge list where the cell has a bridge), reading each successor after
    /// the call:
    /// - an infantryman, not under the Iron Curtain, when the owner is one
    ///   too: killed when his coordinate is exactly `coord`, otherwise left;
    /// - any other Foot not under the Iron Curtain: killed;
    /// - anything under the Iron Curtain: the owner is killed;
    /// - anything else (a building, a tree): the cell is blocked.
    ///
    /// The kills take the victim's authored Strength as `C4Warhead=` damage
    /// with no attacker or house, ignoring defenses. A bridge cell without
    /// `+0x140 & 0x200` is blocked too. A blocked cell moves `+0x288` to the
    /// nearby passable cell (Track, the MovementZone with Fly and Destroyer
    /// read as Normal and AmphibiousDestroyer as Amphibious, the zone of the
    /// owner's own cell with the bridge test on, the target cell's bridge
    /// bit), keeping its offset from the cell's coordinate, and answers
    /// false. Otherwise one raw PUT at the Marked coordinate, and true. (The
    /// cell `+0x44` table test at `0x007184CE` cannot block: its other operand
    /// is a function address, always nonzero.)
    pub(crate) fn chrono_update_position(
        &mut self,
        id: u64,
        coord: DriveCoord,
        place: bool,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> bool {
        let Some(entity) = self.substrate.entities.get(id) else {
            return false;
        };
        let location = super::ground_pose::object_location(entity, self.resolved_terrain.as_ref());
        let marked = entity
            .locomotor
            .as_ref()
            .and_then(|locomotor| locomotor.teleport_runtime())
            .and_then(|runtime| runtime.resolved_destination())
            .unwrap_or(location);
        if place {
            Self::object_raw_receiver_at(
                &mut self.substrate,
                &self.path_grid,
                &self.resolved_terrain,
                id,
                marked,
                false,
            );
            let (ground, bridge) = self
                .resolved_terrain
                .as_ref()
                .map_or((0, false), |terrain| {
                    let cells = NativeCellQuery::canonical(terrain);
                    let ground =
                        super::ground_pose::query_ground_height(&cells, coord).unwrap_or(0);
                    let cell = cells.lookup_world(coord.x, coord.y);
                    (ground, cells.flags(cell) & CELL_FLAG_BRIDGE != 0)
                });
            let Some(entity) = self.substrate.entities.get_mut(id) else {
                return false;
            };
            let on_bridge = bridge && !entity.on_bridge();
            entity.on_bridge.store(on_bridge);
            let z = if on_bridge {
                ground.wrapping_add(crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS)
            } else {
                ground
            };
            let marked = DriveCoord {
                x: coord.x,
                y: coord.y,
                z,
            };
            if let Some(locomotor) = entity.locomotor.as_mut() {
                locomotor.store_ground_layer(on_bridge);
                if let Some(runtime) = locomotor.teleport_runtime_mut() {
                    runtime.set_resolved_destination(marked);
                }
            }
            Self::object_raw_receiver_at(
                &mut self.substrate,
                &self.path_grid,
                &self.resolved_terrain,
                id,
                marked,
                true,
            );
            Self::object_raw_receiver_at(
                &mut self.substrate,
                &self.path_grid,
                &self.resolved_terrain,
                id,
                marked,
                true,
            );
            return true;
        }
        let owner_infantry = entity.category() == EntityCategory::Infantry;
        let target = (coord.x / 256, coord.y / 256);
        let mut blocked = false;
        if let Some((cell, layer)) = crate::sim::superweapon::cell_grid::selected_cell_list(
            self,
            target.0 as i16,
            target.1 as i16,
        ) {
            let mut next = self.cell_objects(cell, layer).next();
            while let Some(member) = next {
                self.chrono_telefrag(
                    id,
                    owner_infantry,
                    member,
                    coord,
                    &mut blocked,
                    rules,
                    registry,
                    effects,
                );
                next = self.next_cell_object(member);
            }
        }
        if let Some(terrain) = self.resolved_terrain.as_ref() {
            let cells = NativeCellQuery::canonical(terrain);
            let flags = cells.flags(cells.lookup_world(coord.x, coord.y));
            if flags & CELL_FLAG_BRIDGE != 0 && flags & CELL_FLAG_BRIDGE_DECK == 0 {
                blocked = true;
            }
        }
        if blocked {
            self.chrono_retarget_blocked(id, coord, location, rules);
            return false;
        }
        Self::object_raw_receiver_at(
            &mut self.substrate,
            &self.path_grid,
            &self.resolved_terrain,
            id,
            marked,
            true,
        );
        true
    }

    /// One object of Update_Position's walk (`0x007182DC..0x0071842A`).
    fn chrono_telefrag(
        &mut self,
        id: u64,
        owner_infantry: bool,
        member: CellObjectMember,
        coord: DriveCoord,
        blocked: &mut bool,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        let CellObjectMember::Entity(other) = member else {
            // A TerrainClass is no Foot and never under the Iron Curtain.
            *blocked = true;
            return;
        };
        let frame = self.session.binary_frame();
        let Some(entity) = self.substrate.entities.get(other) else {
            return;
        };
        let iron_curtained = crate::sim::superweapon::invulnerability::is_invulnerable(
            entity.invulnerability.as_ref(),
            frame,
        );
        if !iron_curtained && entity.category() == EntityCategory::Infantry && owner_infantry {
            let at = super::ground_pose::object_get_coords(entity, self.resolved_terrain.as_ref());
            if at == coord {
                self.chrono_c4_kill(other, rules, registry, effects);
            }
        } else if !iron_curtained && foot(entity.category()) {
            self.chrono_c4_kill(other, rules, registry, effects);
        } else if iron_curtained {
            self.chrono_c4_kill(id, rules, registry, effects);
        } else {
            *blocked = true;
        }
    }

    /// `ReceiveDamage(&Strength, 0, C4Warhead, NULL, true, false, NULL)` on
    /// `victim`: Update_Position's kills (`0x0071836E`, `0x007183C8`,
    /// `0x0071840F`) and PostWarpValidation's (`0x00718804`, `0x00718B53`).
    fn chrono_c4_kill(
        &mut self,
        victim: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        let Some(strength) = self
            .substrate
            .entities
            .get(victim)
            .and_then(|entity| self.object_type(entity.type_ref(), rules))
            .map(|object| object.strength)
        else {
            return;
        };
        let event = crate::sim::combat::EntityDamageEvent::direct_receiver(
            victim,
            strength,
            0,
            crate::sim::combat::RAD_NO_ATTACKER,
            None,
            self.interner.intern(&rules.bridge_warheads.c4_name),
            crate::sim::combat::ReceiverCallFlags {
                ignore_defenses: true,
                arg6: false,
            },
        );
        self.commit_direct_damage_receiver(rules, registry, event, effects);
    }

    /// Update_Position's blocked arm (`0x007184FA..0x00718658`): `+0x288`
    /// moves to the nearby passable cell, keeping `coord`'s offset from its
    /// own cell's coordinate (CellClass vt+0x48). No cell found reads cell
    /// (0, 0), as the search's miss cell (`0x00ABD480`) holds.
    fn chrono_retarget_blocked(
        &mut self,
        id: u64,
        coord: DriveCoord,
        location: DriveCoord,
        rules: &RuleSet,
    ) {
        let Some(movement_zone) = self
            .substrate
            .entities
            .get(id)
            .and_then(|entity| self.object_type(entity.type_ref(), rules))
            .map(|object| match object.movement_zone {
                MovementZone::Fly | MovementZone::Destroyer => MovementZone::Normal,
                MovementZone::AmphibiousDestroyer => MovementZone::Amphibious,
                zone => zone,
            })
        else {
            return;
        };
        let target = (coord.x / 256, coord.y / 256);
        let current = (location.x / 256, location.y / 256);
        let (zone, bridge) = self
            .resolved_terrain
            .as_ref()
            .map_or((None, false), |terrain| {
                let cells = NativeCellQuery::canonical(terrain);
                let bridge = cells.flags(cells.lookup((target.0 as i16, target.1 as i16)))
                    & CELL_FLAG_BRIDGE
                    != 0;
                let zone = self.zone_grid.as_ref().and_then(|zones| {
                    zones.get_zone_id_native(
                        terrain,
                        (current.0 as u16, current.1 as u16),
                        movement_zone,
                        true,
                    )
                });
                (zone, bridge)
            });
        let found = self
            .nearby_passable_cell(
                target,
                NearbyArgs::plain(
                    PassabilityArgs {
                        speed_type: SpeedType::Track,
                        required_zone_id: zone,
                        movement_zone,
                        bridge_aware_zone: bridge,
                    },
                    NearbyFootprint::SINGLE,
                ),
            )
            .unwrap_or((0, 0));
        let terrain = self.resolved_terrain.as_ref();
        let from =
            crate::sim::projectile::cell_ground_coord(terrain, target.0 as u16, target.1 as u16);
        let to = crate::sim::projectile::cell_ground_coord(terrain, found.0, found.1);
        let destination = DriveCoord {
            x: to.x.wrapping_add(coord.x.wrapping_sub(from.x)),
            y: to.y.wrapping_add(coord.y.wrapping_sub(from.y)),
            z: to.z.wrapping_add(coord.z.wrapping_sub(from.z)),
        };
        if let Some(warp) = self.substrate.chrono_warp_mut(id) {
            warp.set_destination(destination);
        }
    }

    /// `TeleportLocomotionClass::PostWarpValidation @ 0x007187A0` on the
    /// Marked coordinate `coord`, at state 5:
    /// 1. anything under the Iron Curtain on the cell's ground list (`+0xE4`)
    ///    kills the owner, reading each successor after the call;
    /// 2. a Naval type is out of place on a bridge cell or a Road cell
    ///    (LandType 1, `+0xEC`); a Hover type (SpeedType 3) is at home on
    ///    water unless it is `PoweredUnit=` and its house has no powered
    ///    center (`0x00718864..0x007188AF`, `sim::powered_unit`);
    /// 3. on a Water cell (LandType 2) with no bridge, an object that is not
    ///    Hover, Naval or Infantry sinks, its slaves go free, and its kill is
    ///    booked for the warping house (`0x00718968..0x007189F4`);
    /// 4. otherwise, when its own `Can_Enter_Cell(cell, -1, -1, NULL, 1)`
    ///    answers 7, or a Naval type is out of place: on a water tile
    ///    (`0x004865D0`) a non-infantry object sinks and its slaves go free,
    ///    with no kill booked (`0x00718ABF..0x00718B1B`); anything else is
    ///    killed (`0x00718B53`).
    fn chrono_post_warp_validation(
        &mut self,
        id: u64,
        coord: DriveCoord,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        let Some(terrain) = self.resolved_terrain.as_ref() else {
            return;
        };
        let cells = NativeCellQuery::canonical(terrain);
        let cell = cells.lookup_world(coord.x, coord.y);
        let (cx, cy) = cells.coord(cell);
        let flags = cells.flags(cell);
        let land = cells.land_type(cell);
        let water_tile = terrain.native_cell_is_water_tile(cell);
        if matches!(cell, crate::map::cell_index::NativeCellIdentity::Real(_)) {
            let frame = self.session.binary_frame();
            let mut next = self
                .cell_objects((cx as u16, cy as u16), MovementLayer::Ground)
                .next();
            while let Some(member) = next {
                if let CellObjectMember::Entity(other) = member
                    && self.substrate.entities.get(other).is_some_and(|entity| {
                        crate::sim::superweapon::invulnerability::is_invulnerable(
                            entity.invulnerability.as_ref(),
                            frame,
                        )
                    })
                {
                    self.chrono_c4_kill(id, rules, registry, effects);
                }
                next = self.next_cell_object(member);
            }
        }
        let Some((category, naval, hover)) = self.substrate.entities.get(id).and_then(|entity| {
            self.object_type(entity.type_ref(), rules).map(|object| {
                (
                    entity.category(),
                    object.naval,
                    object.speed_type == SpeedType::Hover
                        && !(object.powered_unit && !self.has_powered_center(entity.owner())),
                )
            })
        }) else {
            return;
        };
        let road = land == LandType::Road.as_index() as i32;
        let bridge = flags & CELL_FLAG_BRIDGE != 0;
        let naval_out_of_place = naval && (bridge || road);
        let infantry = category == EntityCategory::Infantry;
        if land == LandType::Water.as_index() as i32 && !hover && !naval && !infantry && !bridge {
            self.chrono_warp_sink(id, rules, registry, effects);
            // `0x007189BE..0x007189F5`: the kill goes to the Magnetron that
            // dropped the object, else the warping house.
            self.record_manipulated_kill(
                id,
                id,
                crate::sim::world::UninitContext::new(Some(rules), registry).with_effects(effects),
            );
            return;
        }
        let code = self
            .mover_can_enter(
                id,
                (cx, cy),
                InfantryEntryArgs {
                    direction: -1,
                    height: -1,
                    previous_cell: None,
                },
                EntryQueryMode::CheckLocomotor,
                rules,
                registry,
            )
            .unwrap_or_else(|error| {
                log::debug!("chrono warp {id} entry at ({cx}, {cy}): {error}");
                0
            });
        if code != 7 && !naval_out_of_place {
            return;
        }
        if water_tile && !infantry {
            self.chrono_warp_sink(id, rules, registry, effects);
        } else {
            self.chrono_c4_kill(id, rules, registry, effects);
        }
    }

    /// PostWarpValidation's sink: `+0x3CD` and the Stun, then FreeSlaves with
    /// the dropping Magnetron (`+0x428`) and the warping house (`+0x42C`), and
    /// the slave manager deleted (`0x00718968..0x007189B4`).
    fn chrono_warp_sink(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        self.begin_living_ship_sinking(
            id,
            crate::sim::world::LivingSinkTrigger::PostWarpValidation,
            rules,
            effects,
        );
        let Some((killer, house)) = self.substrate.entities.get(id).map(|entity| {
            (
                entity.drop_credit.manipulated_by(),
                entity.drop_credit.warped_by_house(),
            )
        }) else {
            return;
        };
        self.free_slaves(id, killer, house, rules, registry, effects);
    }
}

impl crate::sim::world::ObjectSubstrate {
    /// The warp of the Teleport running this Process, which the Unit
    /// setter may have suspended under a Drive by now.
    fn chrono_warp_mut(&mut self, id: u64) -> Option<&mut ChronoWarp> {
        self.entities
            .get_mut(id)?
            .locomotor
            .as_mut()?
            .warp_teleport_mut()?
            .chrono_mut()
    }
}
