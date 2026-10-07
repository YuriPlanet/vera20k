//! The Teleport's Chronosphere states: `TeleportLocomotionClass::Process @
//! 0x007192F0` once a Chrono Warp (`superweapon::chronosphere`, case 4)
//! latched its owner (Techno `+0x27C`, `0x00719351`) or its state (`+0x38`)
//! is past 0 (`0x0071935F`), with TimerCheck (`0x00719BF0`), Update_Position
//! (`0x00718260`) and PostWarpValidation (`0x007187A0`).
//!
//! One call of the Process runs one state:
//! - 0: BeingWarpedOut (`+0x270`) and a 60-frame timer; -> 1
//!   (`0x007197D0..0x007197F4`).
//! - 1 and 6: [TimerCheck](Simulation::chrono_timer_check); its expiry
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
//!   Is_Moving down; -> 0 (`0x00719BB4..0x00719BDF`), after which the Foot AI
//!   tail ends the piggyback (`locomotor_owner::piggyback_end_admitted`).
//!
//! FootClass::AI's Process call runs it, and so does each class AI's prologue
//! while the object is warping in, or warped out with the latch
//! (`Simulation::temporal_ai_prologue`): state 1 freezes the object, and the
//! warp itself (states 2 to 4) takes both calls of one frame.
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
//! - The chrono reinforcement (`0x0065EC30`, reached from a trigger action
//!   and the ChronoInTeam team script per their Ghidra labels) is not
//!   ported. It raises WarpingIn, sets Techno `+0x284` to
//!   `[General] ChronoReinfDelay=` (`0x0065F212`) and Techno `+0x280` to 3
//!   (`0x0065F29F`), which state 0 jumps to (`0x00719338..0x00719342`) and
//!   state 5 tests (`0x00719AA3`); state 7 clears it (`0x00719BD9`). VERA
//!   has no other writer, so those branches and the WarpingIn-at-state-0 arm
//!   (`0x00719304..0x00719325`) stay dormant. Trigger: a map's chrono
//!   reinforcements. Effect: they don't arrive (VERA has no reinforcement
//!   action); porting them needs these branches.
//! - A landed Aircraft's TimerCheck idle-mode entry does nothing:
//!   `queue_foot_enter_idle_mode` has no Aircraft arm (its residual).
//!   Trigger: a landed Aircraft in the source block. Effect: it keeps its
//!   mission after the warp.
//! - PostWarpValidation's hover arm (`0x00718864..0x007188AF`): a Hover type
//!   with `PoweredUnit=` (TechnoType `+0x410`) whose house has no matching
//!   powering building (`HouseClass @ 0x0050E1B0`) loses its hover; VERA
//!   reads neither, so every Hover type keeps it. Trigger: a Robot Tank
//!   chronoshifted onto water while its owner has no Robot Control Center.
//!   Effect: it floats instead of sinking.
//! - A landed Aircraft that sinks keeps `+0x3CD` with nothing to end it:
//!   VERA ports only the Unit's sinking (Unit AI `0x007364A1`); the Aircraft
//!   AI's handling is not established. Trigger: an airfield's Harriers
//!   chronoshifted onto water.

use crate::map::entities::EntityCategory;
use crate::map::overlay_types::OverlayTypeRegistry;
use crate::map::resolved_terrain::NativeCellQuery;
use crate::rules::locomotor_type::{MovementZone, SpeedType};
use crate::rules::ruleset::RuleSet;
use crate::rules::terrain_rules::LandType;
use crate::sim::components::DriveCoord;
use crate::sim::find_nearby_cell::PassabilityArgs;
use crate::sim::movement::infantry_entry::{EntryQueryMode, InfantryEntryArgs};
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::movement::teleport_movement::{ChronoWarp, WarpSound};
use crate::sim::occupancy::CellObjectMember;
use crate::sim::world::{FrameAdvanceError, ObjectAiCtx, Simulation};

/// State 0's timer (`0x007197DF`, `MOV ECX,0x3C`).
const CHRONO_WARP_OUT_FRAMES: i32 = 0x3C;
/// CellClass `+0x140`: a bridge over the cell, and the bit Update_Position
/// requires with it (`0x00718452..0x0071846E`).
const CELL_FLAG_BRIDGE: u32 = 0x100;
const CELL_FLAG_BRIDGE_DECK: u32 = 0x200;

fn foot(category: EntityCategory) -> bool {
    matches!(
        category,
        EntityCategory::Unit | EntityCategory::Infantry | EntityCategory::Aircraft
    )
}

impl Simulation {
    fn chrono_warp_mut(&mut self, id: u64) -> Option<&mut ChronoWarp> {
        self.substrate
            .entities
            .get_mut(id)?
            .locomotor
            .as_mut()?
            .teleport_runtime_mut()?
            .chrono_mut()
    }

    /// Object `+0x90`.
    fn chrono_owner_alive(&self, id: u64) -> bool {
        self.substrate
            .entities
            .get(id)
            .is_some_and(|entity| !entity.dying && entity.lifecycle.object_alive)
    }

    /// The Teleport Process of an owner whose Teleport holds a Chronosphere
    /// warp: the state it is in (module docs). Answers whether state 5's
    /// `Per_Cell_Process(2)` changed a bridge's state.
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
        let frame = self.session.binary_frame;
        match warp.state() {
            0 => {
                if let Some(warp) = self.chrono_warp_mut(id) {
                    warp.set_bytes(true, true, false);
                    warp.start_timer(frame, CHRONO_WARP_OUT_FRAMES);
                    warp.set_state(1);
                }
            }
            1 | 6 => self.chrono_timer_check(id, rules, ctx),
            2 => {
                self.teleport_warp_out(id, rules);
                self.foot_mark_remove(id, Some(rules), registry);
                self.teleport_warp_sound(id, WarpSound::Out, rules);
                if let Some(entity) = self.substrate.entities.get_mut(id) {
                    entity.on_bridge = false;
                    if let Some(locomotor) = entity.locomotor.as_mut() {
                        locomotor.layer = MovementLayer::Ground;
                    }
                }
                if let Some(warp) = self.chrono_warp_mut(id) {
                    warp.set_bytes(false, false, true);
                }
                let landed =
                    self.chrono_update_position(id, warp.destination(), false, rules, registry);
                if let Some(warp) = self.chrono_warp_mut(id) {
                    warp.set_state(if landed { 4 } else { 3 });
                }
            }
            3 => {
                let landed =
                    self.chrono_update_position(id, warp.destination(), false, rules, registry);
                if landed && let Some(warp) = self.chrono_warp_mut(id) {
                    warp.set_state(4);
                }
                if let Some(entity) = self.substrate.entities.get_mut(id) {
                    entity.set_chrono_warp_delay(rules.general.chrono_delay);
                }
            }
            4 => {
                self.chrono_update_position(id, warp.destination(), true, rules, registry);
                self.chrono_settle(id, rules, registry);
                if let Some(warp) = self.chrono_warp_mut(id) {
                    warp.set_state(5);
                }
            }
            5 => {
                let marked = self.chrono_settle(id, rules, registry);
                self.teleport_warp_sound(id, WarpSound::In, rules);
                self.chrono_playfield_check(id);
                self.chrono_post_warp_validation(id, marked, rules, registry);
                if !self.chrono_owner_alive(id) {
                    return Ok(bridge_state_changed);
                }
                bridge_state_changed |= self.per_cell_process(
                    id,
                    super::PerCellReason::Arrival,
                    Some(rules),
                    registry,
                )?;
                if let Some(entity) = self.substrate.entities.get_mut(id) {
                    super::teleport_movement::teleport_stop_moving(entity);
                    entity.set_archive_target(None);
                }
                if let Some(warp) = self.chrono_warp_mut(id) {
                    warp.clear_house();
                }
                self.assign_null_destination(id, Some(rules), registry);
                let delay = self
                    .substrate
                    .entities
                    .get(id)
                    .map_or(0, |entity| entity.chrono_warp_delay());
                if let Some(warp) = self.chrono_warp_mut(id) {
                    warp.start_timer(frame, delay);
                }
                self.teleport_warp_out(id, rules);
                if let Some(warp) = self.chrono_warp_mut(id) {
                    warp.set_state(6);
                }
            }
            7 => {
                if let Some(warp) = self.chrono_warp_mut(id) {
                    warp.set_warping_in(false);
                }
                if let Some(entity) = self.substrate.entities.get_mut(id) {
                    entity.set_archive_target(None);
                }
                self.assign_null_destination(id, Some(rules), registry);
                if let Some(runtime) = self
                    .substrate
                    .entities
                    .get_mut(id)
                    .and_then(|entity| entity.locomotor.as_mut())
                    .and_then(|locomotor| locomotor.teleport_runtime_mut())
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
    ) -> DriveCoord {
        let marked = self
            .substrate
            .entities
            .get(id)
            .and_then(|entity| entity.locomotor.as_ref())
            .and_then(|locomotor| locomotor.teleport_runtime())
            .and_then(|runtime| runtime.resolved_destination())
            .unwrap_or(DriveCoord { x: 0, y: 0, z: 0 });
        self.foot_set_location_marked(id, marked, Some(rules), registry);
        self.set_object_height(id, 0, Some(rules), registry);
        self.foot_mark_put(id, Some(rules), registry);
        marked
    }

    /// State 5's playfield test (`0x00719A75..0x00719A99`): the cell of the
    /// Location (vt+0x1B8) outside `MapClass::IsCellInPlayfield(cell, 1)`
    /// clears `+0x3D5`; inside, the byte stays as it was.
    fn chrono_playfield_check(&mut self, id: u64) {
        let Some(cell) = self
            .substrate
            .entities
            .get(id)
            .map(|entity| (i32::from(entity.position.rx), i32::from(entity.position.ry)))
        else {
            return;
        };
        if !crate::sim::cell_rect::cell_is_in_playfield_height_aware(
            cell,
            self.playfield_bounds,
            self.resolved_terrain.as_ref(),
        ) && let Some(entity) = self.substrate.entities.get_mut(id)
        {
            entity.in_playfield = false;
        }
    }

    /// `TeleportLocomotionClass::TimerCheck @ 0x00719BF0`: once the timer has
    /// run out, WarpingIn drops (`0x00719C15`); an owner holding no target
    /// (`+0x2B4`) shortens its passive scan timer (`0x0070F770`) and scans
    /// ([`passive_target_acquire`](crate::sim::world::passive_target_acquire),
    /// `0x00709480`), entering idle mode (vt+0x484(0, 1)) when that finds
    /// nothing; then a positive state advances (`0x00719C49..0x00719C51`).
    fn chrono_timer_check(&mut self, id: u64, rules: &RuleSet, ctx: ObjectAiCtx<'_>) {
        let frame = self.session.binary_frame as i32;
        let Some(warp) = self.chrono_warp_mut(id) else {
            return;
        };
        if !warp.timer().expired(frame) {
            return;
        }
        warp.set_warping_in(false);
        if self
            .substrate
            .entities
            .get(id)
            .is_some_and(|entity| entity.attack_target.is_none())
        {
            self.shorten_passive_scan_timer(id);
            if !crate::sim::world::passive_target_acquire(self, id, rules, ctx) {
                crate::sim::world::queue_foot_enter_idle_mode(self, id, rules);
            }
        }
        if let Some(warp) = self.chrono_warp_mut(id)
            && warp.state() > 0
        {
            warp.set_state(warp.state() + 1);
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
            self.object_raw_receiver_at(id, marked, false);
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
            let on_bridge = bridge && !entity.on_bridge;
            entity.on_bridge = on_bridge;
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
                locomotor.layer = if on_bridge {
                    MovementLayer::Bridge
                } else {
                    MovementLayer::Ground
                };
                if let Some(runtime) = locomotor.teleport_runtime_mut() {
                    runtime.set_resolved_destination(marked);
                }
            }
            self.object_raw_receiver_at(id, marked, true);
            self.object_raw_receiver_at(id, marked, true);
            return true;
        }
        let owner_infantry = entity.category == EntityCategory::Infantry;
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
        self.object_raw_receiver_at(id, marked, true);
        true
    }

    /// One object of Update_Position's walk (`0x007182DC..0x0071842A`).
    #[allow(clippy::too_many_arguments)]
    fn chrono_telefrag(
        &mut self,
        id: u64,
        owner_infantry: bool,
        member: CellObjectMember,
        coord: DriveCoord,
        blocked: &mut bool,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) {
        let CellObjectMember::Entity(other) = member else {
            // A TerrainClass is no Foot and never under the Iron Curtain.
            *blocked = true;
            return;
        };
        let frame = self.session.binary_frame;
        let Some(entity) = self.substrate.entities.get(other) else {
            return;
        };
        let iron_curtained = crate::sim::superweapon::invulnerability::is_invulnerable(
            entity.invulnerability.as_ref(),
            frame,
        );
        if !iron_curtained && entity.category == EntityCategory::Infantry && owner_infantry {
            let at = super::ground_pose::object_get_coords(entity, self.resolved_terrain.as_ref());
            if at == coord {
                self.chrono_c4_kill(other, rules, registry);
            }
        } else if !iron_curtained && foot(entity.category) {
            self.chrono_c4_kill(other, rules, registry);
        } else if iron_curtained {
            self.chrono_c4_kill(id, rules, registry);
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
        self.commit_direct_damage_receiver(rules, registry, event);
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
            .nearby_location_cell(
                target,
                PassabilityArgs {
                    speed_type: SpeedType::Track,
                    required_zone_id: zone,
                    movement_zone,
                    bridge_aware_zone: bridge,
                },
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
        if let Some(warp) = self.chrono_warp_mut(id) {
            warp.set_destination(destination);
        }
    }

    /// `TeleportLocomotionClass::PostWarpValidation @ 0x007187A0` on the
    /// Marked coordinate `coord`, at state 5:
    /// 1. anything under the Iron Curtain on the cell's ground list (`+0xE4`)
    ///    kills the owner, reading each successor after the call;
    /// 2. a Naval type is out of place on a bridge cell or a Road cell
    ///    (LandType 1, `+0xEC`); a Hover type (SpeedType 3) is at home on
    ///    water (module residual);
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
            let frame = self.session.binary_frame;
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
                    self.chrono_c4_kill(id, rules, registry);
                }
                next = self.next_cell_object(member);
            }
        }
        let Some((category, naval, hover)) = self.substrate.entities.get(id).and_then(|entity| {
            self.object_type(entity.type_ref(), rules).map(|object| {
                (
                    entity.category,
                    object.naval,
                    object.speed_type == SpeedType::Hover,
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
            self.chrono_warp_sink(id, rules, registry);
            let house = self
                .substrate
                .entities
                .get(id)
                .and_then(|entity| entity.chrono_warp())
                .and_then(ChronoWarp::house);
            if let Some(house) = house {
                // TechnoClass vt+0xE4 (`0x00703230`): Record_The_Kill's
                // house-credited twin, with no experience to award.
                self.record_the_kill(
                    id,
                    None,
                    Some(house),
                    crate::sim::combat::KillCallback::Terminal,
                    rules,
                );
            }
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
            self.chrono_warp_sink(id, rules, registry);
        } else {
            self.chrono_c4_kill(id, rules, registry);
        }
    }

    /// PostWarpValidation's sink: `+0x3CD` and the Stun, then FreeSlaves with
    /// `+0x428` (VERA keeps none) and the warping house, and the slave
    /// manager deleted (`0x00718968..0x007189B4`).
    fn chrono_warp_sink(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) {
        self.begin_warp_sinking(id, rules);
        let house = self
            .substrate
            .entities
            .get(id)
            .and_then(|entity| entity.chrono_warp())
            .and_then(ChronoWarp::house);
        self.free_slaves(id, None, house, rules, registry);
    }
}
