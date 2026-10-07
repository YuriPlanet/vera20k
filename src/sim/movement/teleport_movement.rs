//! Teleport (chrono) locomotor — instant relocation with chrono delay.
//!
//! Implements the Teleport state machine for chrono-style movement:
//! Relocate (instant, one frame) → ChronoDelay (being_warped countdown) → Idle.
//!
//! Self-teleport relocates the unit in a single frame (Phase 0), then the unit
//! sits at the destination 50% translucent for `chrono_delay` frames until fully
//! materialized.
//!
//! Units with `Locomotor=Teleport` use this. A `Teleporter=` unit (the Chrono
//! Miner, whose primary locomotor is this one) drives instead wherever the
//! Unit setter's Teleporter arm (`0x007423CD`, `movement/track_path.rs`)
//! installs a Drive piggyback: every destination except a cell MOVE_HERE
//! while radio slot 0 is a `DockUnload=` building.
//!
//! No pathfinding — the unit is relocated instantly. The object turn runs the
//! rest of the warp around the relocation in native order (detach sweep,
//! departure WarpOut, parasite eject, Mark(UP), then after it Mark(DOWN),
//! `Per_Cell_Process(2)`, the NULL assign and the arrival WarpOut).
//!
//! ## Dependency rules
//! - Part of sim/ — depends on sim/game_entity, sim/entity_store, sim/locomotor.
//! - sim/ NEVER depends on render/, ui/, sidebar/, audio/, net/.

use crate::rules::locomotor_type::LocomotorKind;
use crate::rules::ruleset::GeneralRules;
use crate::sim::components::AnimClassSpawnDescriptor;
use crate::sim::components::DriveCoord;
use crate::sim::debug_event_log::DebugEventKind;
use crate::sim::entity_store::EntityStore;
use crate::sim::intern::InternedId;
use crate::util::fixed_math::isqrt_i64;

const TELEPORT_WARP_DRAW_FLAGS: u32 = 0x600;

/// The warp's two VocClass::PlayAt calls at the owner's Location: ChronoOut
/// (the ordinary warp `0x0071962C`, the Chronosphere's state 2 `0x007198CE`)
/// and ChronoIn (`0x00719710`, state 5 `0x00719A70`).
#[derive(Clone, Copy)]
pub(crate) enum WarpSound {
    Out,
    In,
}

impl crate::sim::world::Simulation {
    /// One warp sound at the owner's cell: the type's
    /// ChronoOutSound/ChronoInSound (TechnoType+0x578/+0x574), else
    /// `[AudioVisual]` (Rules+0x21C/+0x218), else silence.
    pub(crate) fn teleport_warp_sound(
        &mut self,
        id: u64,
        sound: WarpSound,
        rules: &crate::rules::ruleset::RuleSet,
    ) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        let (rx, ry) = (entity.position.rx, entity.position.ry);
        let object = self.object_type(entity.type_ref(), rules);
        let name = match sound {
            WarpSound::Out => object
                .and_then(|object| object.chrono_out_sound.clone())
                .or_else(|| rules.general.chrono_out_sound.clone()),
            WarpSound::In => object
                .and_then(|object| object.chrono_in_sound.clone())
                .or_else(|| rules.general.chrono_in_sound.clone()),
        };
        if let Some(name) = name {
            let sound_id = self.interner.intern(&name);
            self.sound_events
                .push(crate::sim::world::SimSoundEvent::ChronoTeleport { sound_id, rx, ry });
        }
    }

    /// `AnimClass([General] WarpOut=, Location, 0, 1, 0x600, 0, 0)` at the
    /// owner's exact Location (`+0x9C`), constructed inside the mover's own
    /// turn: the ordinary warp's departure and arrival (`0x00719442`,
    /// `0x00719791`) and the Chronosphere's states 2 and 5 (`0x00719873`,
    /// `0x00719B7C`).
    pub(crate) fn teleport_warp_out(&mut self, id: u64, rules: &crate::rules::ruleset::RuleSet) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        let location = super::ground_pose::object_location(entity, self.resolved_terrain.as_ref());
        let world = crate::sim::anim_class::AnimWorldCoord {
            x: location.x,
            y: location.y,
            z: location.z,
        };
        let (rx, ry, sub_x, sub_y, z) = world.to_cell_sub_z();
        let type_id = self.interner.intern(&rules.general.warp_out.name);
        let descriptor = AnimClassSpawnDescriptor {
            delay: 0,
            loop_count: 1,
            draw_flags: TELEPORT_WARP_DRAW_FLAGS,
            z_adjust: 0,
            reverse: false,
            ..AnimClassSpawnDescriptor::new(type_id, rx, ry, sub_x, sub_y, z)
        };
        if let Err(error) = self.spawn_anim_at_world(rules, descriptor, world) {
            // An art type that never bound draws nothing natively either; see
            // `spawn_combat_explosion_anim`.
            log::debug!(
                "teleport warp [{}] did not construct: {error}",
                rules.general.warp_out.name
            );
        }
    }
}

/// Phase within the teleport state machine.
///
/// Phase 0 relocates instantly in one frame, then the chrono delay timer
/// counts down while the unit is semi-transparent at the destination.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum TeleportPhase {
    /// Instant relocation between the owner's Mark(UP) and Mark(DOWN). Executes in
    /// one frame, then transitions to ChronoDelay.
    Relocate,
    /// Post-warp chrono delay: unit sits at destination 50% translucent,
    /// `being_warped_ticks` counts down each frame. When it reaches 0 the
    /// teleport is complete and the base locomotor is restored.
    ChronoDelay,
}

/// Per-frame result returned by the special locomotor Process adapters.
///
/// The native Process vtable slot owns the completion return; keeping that
/// result explicit prevents callers from inferring completion from an absent
/// movement target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpecialMovementOutcome {
    Continue,
    Complete,
    Abort,
}

/// State for an in-progress teleport.
///
/// Set by `teleport_move_to()` and cleared when the chrono delay
/// expires. The render system reads `being_warped_ticks` to apply 50%
/// translucency while the unit materializes.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct TeleportState {
    phase: TeleportPhase,
    /// Armed XYZ (+1C in the complete native object), independently nullable
    /// from the retained resolver coordinate. No cell-centre mirror is stored.
    destination: Option<DriveCoord>,
    being_warped_ticks: u32,
}

impl TeleportState {
    pub fn phase(&self) -> TeleportPhase {
        self.phase
    }

    pub fn being_warped_ticks(&self) -> u32 {
        self.being_warped_ticks
    }

    #[cfg(test)]
    pub(crate) fn destination(&self) -> Option<DriveCoord> {
        self.destination
    }

    #[cfg(test)]
    pub(crate) fn target_cell(&self) -> Option<(u16, u16)> {
        self.destination
            .map(|coord| ((coord.x / 256) as u16, (coord.y / 256) as u16))
    }

    #[cfg(test)]
    pub(crate) fn for_test(phase: TeleportPhase, rx: u16, ry: u16, ticks: u32) -> Self {
        Self {
            phase,
            destination: Some(DriveCoord::cell(rx, ry, 0)),
            being_warped_ticks: ticks,
        }
    }

    #[cfg(test)]
    pub(crate) fn set_phase_for_test(&mut self, phase: TeleportPhase) {
        self.phase = phase;
    }

    #[cfg(test)]
    pub(crate) fn set_destination_for_test(&mut self, destination: DriveCoord) {
        self.destination = Some(destination);
    }

    #[cfg(test)]
    pub(crate) fn set_ticks_for_test(&mut self, ticks: u32) {
        self.being_warped_ticks = ticks;
    }

    /// The warp-in byte (`TechnoClass+0x271`) of the ordinary teleport:
    /// written 1 by the arrival (`0x00719579`), cleared by TimerCheck
    /// (`0x00719BF0`). The ordinary teleport never writes `+0x270`.
    pub fn warp_in_active(&self) -> bool {
        self.phase == TeleportPhase::ChronoDelay && self.being_warped_ticks > 0
    }

    /// The relocation frame is removed from normal targeting before its cell
    /// and occupancy mutation; it becomes targetable again while materializing.
    #[cfg(test)]
    pub fn is_targetable(&self) -> bool {
        self.phase == TeleportPhase::ChronoDelay
    }
}

/// The Chronosphere's warp on the Teleport a Chrono Warp piggybacks over a
/// Foot's locomotor (`SuperClass::Launch 0x006CC989..0x006CCB6A`): Teleport
/// Process's states (`+0x38`, `0x007197CC..0x00719BE2`, run by
/// `movement::teleport_chrono`), its timer, and the Techno bytes those states
/// write. Native keeps the bytes on the Techno; only the Teleport's states
/// write them, and End_Piggyback waits until all are clear
/// (`0x00719F30`), so they live and die with this Teleport.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct ChronoWarp {
    /// Teleport `+0x38`.
    state: u8,
    /// Teleport `+0x3C..+0x44`.
    timer: crate::sim::timer::CdTimer,
    /// Techno `+0x288` ChronoDestCoords.
    destination: DriveCoord,
    /// Techno `+0x42C` ChronoWarpedByHouse.
    house: Option<InternedId>,
    /// Techno `+0x27C`: set by case 4, cleared by state 2.
    latched: bool,
    /// Techno `+0x270` BeingWarpedOut: set by state 0, cleared by state 2.
    warped_out: bool,
    /// Techno `+0x271` WarpingIn: set by state 2, cleared by TimerCheck's
    /// expiry and state 7.
    warping_in: bool,
}

impl ChronoWarp {
    /// Case 4's arming of a fresh Teleport, whose constructor starts its
    /// timer at the current frame with no duration (`0x0071804C..0x00718058`),
    /// with the latch (`0x006CCC3D`), the destination (`0x006CCC48`) and the
    /// Super's owner (`0x006CCC67`).
    pub(crate) fn new(destination: DriveCoord, house: InternedId, frame: u32) -> Self {
        Self {
            state: 0,
            timer: crate::sim::timer::CdTimer::started(frame as i32, 0),
            destination,
            house: Some(house),
            latched: true,
            warped_out: false,
            warping_in: false,
        }
    }

    pub(crate) fn state(&self) -> u8 {
        self.state
    }

    pub(crate) fn set_state(&mut self, state: u8) {
        self.state = state;
    }

    pub(crate) fn timer(&self) -> crate::sim::timer::CdTimer {
        self.timer
    }

    pub(crate) fn start_timer(&mut self, frame: u32, duration: i32) {
        self.timer.start(frame as i32, duration);
    }

    pub(crate) fn destination(&self) -> DriveCoord {
        self.destination
    }

    pub(crate) fn set_destination(&mut self, destination: DriveCoord) {
        self.destination = destination;
    }

    pub(crate) fn house(&self) -> Option<InternedId> {
        self.house
    }

    pub(crate) fn clear_house(&mut self) {
        self.house = None;
    }

    /// Techno `+0x27C`.
    pub(crate) fn latched(&self) -> bool {
        self.latched
    }

    /// Techno `+0x270`.
    pub(crate) fn warped_out(&self) -> bool {
        self.warped_out
    }

    /// Techno `+0x271`.
    pub(crate) fn warping_in(&self) -> bool {
        self.warping_in
    }

    pub(crate) fn set_bytes(&mut self, latched: bool, warped_out: bool, warping_in: bool) {
        self.latched = latched;
        self.warped_out = warped_out;
        self.warping_in = warping_in;
    }

    pub(crate) fn set_warping_in(&mut self, warping_in: bool) {
        self.warping_in = warping_in;
    }
}

/// Teleport718000 owns both the armed destination and resolver718B70's
/// persistent +28 XYZ. Stop718230 clears the request/armed XYZ but retains
/// +28; the next resolver releases that reservation before selecting another.
/// This complete payload travels with BEGIN/END and snapshot persistence.
/// Original Cell-request controls: infantry_scatter_destination --teleport-cell.
#[derive(Debug, Default, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct TeleportRuntime {
    resolved: Option<DriveCoord>,
    requested: bool,
    warp: Option<TeleportState>,
    #[serde(default)]
    chrono: Option<ChronoWarp>,
}

impl TeleportRuntime {
    /// Original718080 reads class+34 ==1, independently of the warp adapter.
    pub(crate) fn is_moving(&self) -> bool {
        self.requested
    }

    pub(crate) fn warp(&self) -> Option<&TeleportState> {
        self.warp.as_ref()
    }

    pub(crate) fn resolved_destination(&self) -> Option<DriveCoord> {
        self.resolved
    }

    pub(crate) fn chrono(&self) -> Option<&ChronoWarp> {
        self.chrono.as_ref()
    }

    pub(crate) fn chrono_mut(&mut self) -> Option<&mut ChronoWarp> {
        self.chrono.as_mut()
    }

    /// Case 4's arming of a fresh Teleport (`0x006CCC3D..0x006CCC67`).
    pub(crate) fn arm_chrono(&mut self, warp: ChronoWarp) {
        self.chrono = Some(warp);
    }

    /// State 7's return to state 0 (`0x00719BB4..0x00719BDF`): with every
    /// byte clear the warp is over. It also drops Is_Moving (`+0x34`).
    pub(crate) fn end_chrono(&mut self) {
        self.chrono = None;
        self.requested = false;
    }

    /// Update_Position's `+0x28` Marked coordinate (`0x007186B9..0x007186E4`).
    pub(crate) fn set_resolved_destination(&mut self, coord: DriveCoord) {
        self.resolved = Some(coord);
    }

    fn stop(&mut self) {
        self.requested = false;
        if let Some(warp) = self.warp.as_mut() {
            warp.destination = None;
            if warp.phase == TeleportPhase::Relocate {
                self.warp = None;
            }
        }
    }
}

impl crate::sim::game_entity::GameEntity {
    /// The Chronosphere warp on the active Teleport.
    pub(crate) fn chrono_warp(&self) -> Option<&ChronoWarp> {
        self.locomotor.as_ref()?.teleport_runtime()?.chrono()
    }

    /// Techno `+0x27C`, the Chronosphere's warp latch.
    pub(crate) fn chrono_warp_latch(&self) -> bool {
        self.chrono_warp().is_some_and(ChronoWarp::latched)
    }

    /// View of the owned Teleport effect, including a suspended instance.
    /// Active locomotor queries and Process use only the active payload.
    pub fn teleport_state(&self) -> Option<&TeleportState> {
        self.locomotor.as_ref()?.teleport_effect_state()
    }

    #[cfg(test)]
    pub(crate) fn install_teleport_state_for_test(&mut self, state: Option<TeleportState>) {
        use super::locomotor::LocomotorState;
        if self
            .locomotor
            .as_ref()
            .is_none_or(|l| !l.has_teleport_instance())
        {
            if state.is_none() {
                return;
            }
            let teleport = LocomotorState::for_test_kind(LocomotorKind::Teleport);
            if let Some(active) = self.locomotor.take() {
                // Supplied Foot warp flags must not reclassify an already
                // active Walk/Drive fixture or discard its paid state. Keep
                // the effect in the same suspended-instance view used above.
                self.locomotor = Some(super::locomotion::piggyback::suspend_effect_for_test(
                    active, teleport,
                ));
            } else {
                self.locomotor = Some(teleport);
            }
        }
        let runtime = self
            .locomotor
            .as_mut()
            .unwrap()
            .teleport_instance_for_test_mut()
            .unwrap();
        runtime.resolved = state.as_ref().and_then(|state| state.destination);
        runtime.requested = state
            .as_ref()
            .is_some_and(|state| state.phase == TeleportPhase::Relocate);
        runtime.warp = state;
    }

    #[cfg(test)]
    pub(crate) fn teleport_state_for_test_mut(&mut self) -> Option<&mut TeleportState> {
        self.locomotor
            .as_mut()?
            .teleport_instance_for_test_mut()?
            .warp
            .as_mut()
    }
}

/// Compute the chrono warp delay in native gameplay frames from distance.
///
/// When `ChronoTrigger=yes`, delay scales linearly with distance in leptons,
/// divided by `ChronoDistanceFactor` (default 48), clamped to at least
/// `ChronoMinimumDelay` (default 16). Short distances below `ChronoRangeMinimum`
/// are forced to the minimum.
pub fn compute_chrono_delay(rules: &GeneralRules, distance_leptons: i32) -> u32 {
    if !rules.chrono_trigger {
        return rules.chrono_minimum_delay.max(0) as u32;
    }
    let mut delay = if rules.chrono_distance_factor > 0 {
        distance_leptons / rules.chrono_distance_factor
    } else {
        0
    };
    if delay < rules.chrono_minimum_delay {
        delay = rules.chrono_minimum_delay;
    }
    if distance_leptons < rules.chrono_range_minimum {
        delay = rules.chrono_minimum_delay;
    }
    delay.max(0) as u32
}

/// Original Teleport MoveTo718100, reached by the Unit/Infantry Foot tail.
/// Ordinary Infantry Cell requests resolve through718B70 ->481180 ->51BF90
/// and the canonical raw receiver. The previous +28 reservation survives Stop
/// and is released before the next selection. Original controls live in
/// infantry_scatter_destination --teleport-cell.
///
/// The legacy Unit Cell resolver and Infantry object/FNPC branches remain
/// bounded adapters. EMP/death admission, full Process and chrono timing are
/// separate residuals; this entry does not certify them. A warped-out or
/// warping-in owner (vt+0x1D4/+0x1D8, `0x0071812E..0x0071814E`) refuses the
/// request and loses its NavCom (`0x0071820F`).
impl crate::sim::world::Simulation {
    pub(crate) fn teleport_move_to(
        &mut self,
        id: u64,
        target: (u16, u16),
        rules: &crate::rules::ruleset::RuleSet,
        is_harvester: bool,
        registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
    ) -> Result<bool, String> {
        use crate::map::entities::EntityCategory;
        use crate::map::resolved_terrain::NativeCellQuery;
        use crate::sim::movement::infantry_entry::InfantryEntryArgs;
        use crate::sim::movement::locomotor::MovementLayer;
        use crate::sim::occupancy::RawCellKey;

        let actor = self
            .substrate
            .entities
            .get(id)
            .ok_or("Teleport MoveTo lost owner")?;
        if actor
            .locomotor
            .as_ref()
            .and_then(|l| l.teleport_runtime())
            .is_none()
        {
            return Ok(false);
        }
        if actor.is_paralyzed(self.session.binary_frame)
            || actor.is_warped_out()
            || actor.is_warping_in()
        {
            self.substrate
                .entities
                .get_mut(id)
                .unwrap()
                .navigation
                .nav_com = None;
            return Ok(false);
        }
        let infantry = actor.category == EntityCategory::Infantry;
        let latched = actor.chrono_warp_latch();
        let physical = super::ground_pose::position_world_coord(&actor.position);
        let marked = actor
            .locomotor
            .as_ref()
            .unwrap()
            .teleport_runtime()
            .unwrap()
            .resolved_destination();
        let previous = marked.unwrap_or(physical);
        // 0x00718B9F..0x00718BCF, 0x0071908D..0x007190A3: a latched owner
        // other than Infantry skips the resolution. After the resolver's
        // REMOVE at Marked-or-Location it PUTs at Marked as it stands (the
        // null coordinate's cell (0,0) without one); the tail
        // (0x00719249..0x007192BB) PUTs at Marked and accepts, or PUTs back at
        // Location and refuses, and Move_To takes the class NULL arm
        // (0x007181F9). The Chrono Warp's fresh Teleport has no Marked
        // coordinate, so its Unit drops the destination it was given.
        if latched && !infantry {
            self.object_raw_receiver_at(id, previous, false);
            self.object_raw_receiver_at(
                id,
                marked.unwrap_or(DriveCoord { x: 0, y: 0, z: 0 }),
                true,
            );
            if let Some(marked) = marked {
                self.object_raw_receiver_at(id, marked, true);
                if let Some(runtime) = self
                    .substrate
                    .entities
                    .get_mut(id)
                    .and_then(|actor| actor.locomotor.as_mut())
                    .and_then(|locomotor| locomotor.teleport_runtime_mut())
                {
                    runtime.requested = true;
                }
                return Ok(true);
            }
            self.object_raw_receiver_at(id, physical, true);
            self.assign_null_destination(id, Some(rules), registry);
            return Ok(false);
        }
        if infantry
            && !self.infantry_destination_inputs_available(
                id,
                crate::sim::components::NavTargetRef::cell(target.0, target.1),
                rules,
                registry,
            )
        {
            return Err("Teleport Infantry resolution requires available class inputs".into());
        }
        let input = super::navcom::target_cell_coord(
            target.0,
            target.1,
            self.resolved_terrain
                .as_ref()
                .map(crate::map::resolved_terrain::NativeCellQuery::canonical)
                .as_ref(),
        );
        let destination = if infantry {
            // Dependency availability belongs to the destination transaction.
            // A direct call also refuses missing map inputs before its first write.
            if self.resolved_terrain.is_none() {
                return Err("Teleport Infantry resolution requires map cells".into());
            }
            let resolution = (|| -> Result<Option<DriveCoord>, String> {
                self.object_raw_receiver_at(id, previous, false);
                let terrain = self.resolved_terrain.as_ref().unwrap();
                let cells = NativeCellQuery::canonical(terrain);
                let cell = cells.lookup_world(input.x, input.y);
                // Original718C23: strict signed comparison, independently of
                // OnBridge. The structural gate precedes the ground query.
                // Original717EC0 startup establishes B0EC38=104.
                let deck = cells.flags(cell) & 0x100 != 0
                    && physical.z
                        > super::ground_pose::query_ground_height(&cells, input)?
                            .wrapping_add(3 * crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS);
                let layer = if deck {
                    MovementLayer::Bridge
                } else {
                    MovementLayer::Ground
                };
                let key = RawCellKey::from_native(terrain, cell);
                // Original718C90 Ready ->718C9F Commence follows the release and
                // deck lookup, before effective-mission reads/Cell481180. Use the
                // existing Mission authority; a queued Attack can become current
                // here after51AA40's earlier current-mission Stop decision.
                if self.mission_ready_to_commence(id, rules) {
                    self.mission_commence_exact(id, self.session.binary_frame)
                        .map_err(|error| format!("Teleport resolver Commence: {error}"))?;
                }
                // Pure Cell NavCom has no object RTTI receiver, so the original
                // Enter/Eaten/Capture/Patrol priority corridor leaves priority0.
                let cells = NativeCellQuery::canonical(self.resolved_terrain.as_ref().unwrap());
                let packed = cells.coord(cell);
                let ground_raw = self
                    .substrate
                    .raw_cell_occupation
                    .bits_at(key, MovementLayer::Ground);
                let selected_raw = self.substrate.raw_cell_occupation.bits_at(key, layer);
                let gate_open = selected_raw & 0x20 == 0
                    && ground_raw & 0x40 != 0
                    && super::bump_crush::ground_gate_is_open(
                        &self.substrate.occupancy,
                        &self.substrate.entities,
                        Some(rules),
                        &self.interner,
                        (packed.0 as u16, packed.1 as u16),
                    );
                let slot = super::bump_crush::place_infantry_in_native_cell(
                    &self.substrate.raw_cell_occupation,
                    key,
                    layer,
                    input,
                    false,
                    gate_open,
                    &mut self.scenario_rng,
                );
                // Cell481180 samples floor at the original input after selection;
                // a failed selection returns NULL without that successful tail.
                let resolved = if let Some(slot) = slot {
                    let floor = super::ground_pose::query_ground_height(&cells, input)?;
                    Some(super::walk_head::selected_head(input, slot, floor, deck))
                } else {
                    None
                };
                self.substrate
                    .entities
                    .get_mut(id)
                    .unwrap()
                    .locomotor
                    .as_mut()
                    .unwrap()
                    .teleport_runtime_mut()
                    .unwrap()
                    .resolved = resolved;
                let point = resolved.unwrap_or(DriveCoord { x: 0, y: 0, z: 0 });
                let cell = NativeCellQuery::canonical(self.resolved_terrain.as_ref().unwrap())
                    .lookup_world(point.x, point.y);
                let answer = self.infantry_can_enter(
                    id,
                    cell,
                    InfantryEntryArgs {
                        direction: -1,
                        height: -1,
                        previous_cell: None,
                    },
                    rules,
                    registry,
                )?;
                Ok(if answer.is_nonzero() { None } else { resolved })
            })();
            let resolved = match resolution {
                Ok(Some(resolved)) => Some(resolved),
                Ok(None) => None,
                Err(error) => {
                    // A missing retained input is not a native admission code.
                    // It must nevertheless leave the raw receiver consistent
                    // and let Foot finish its ordinary refused-request tail.
                    log::debug!("Teleport Infantry resolution {id} refused: {error}");
                    None
                }
            };
            let Some(resolved) = resolved else {
                self.substrate
                    .entities
                    .get_mut(id)
                    .unwrap()
                    .locomotor
                    .as_mut()
                    .unwrap()
                    .teleport_runtime_mut()
                    .unwrap()
                    .resolved = None;
                // Original719286..7192AE restores the physical raw receiver
                // before returning false;7181F9 then calls the class NULL arm.
                self.object_raw_receiver_at(id, physical, true);
                self.assign_null_destination(id, Some(rules), registry);
                return Ok(false);
            };
            self.object_raw_receiver_at(id, resolved, true);
            resolved
        } else {
            // Existing Unit/CMIN behaviour is retained. Its composed resolver,
            // reservation and FNPC admission are outside this Infantry chain.
            input
        };
        Ok(arm_teleport(
            self.substrate.entities.get_mut(id).unwrap(),
            destination,
            target,
            &rules.general,
            is_harvester,
        ))
    }
}

/// `TeleportLocomotionClass::Stop_Moving @ 0x00718230` on the owner's active
/// Teleport: it nulls the armed destination (+0x18..+0x20) and clears the
/// +0x30/+0x32 request bytes. The post-warp delay is the owner's (+0x271),
/// so a warping-in owner keeps it.
pub(crate) fn teleport_stop_moving(entity: &mut crate::sim::game_entity::GameEntity) {
    if let Some(runtime) = entity
        .locomotor
        .as_mut()
        .and_then(|l| l.teleport_runtime_mut())
    {
        runtime.stop();
    }
}

/// Whether the owner's Teleport Process runs this frame: Teleport is its
/// active locomotor. A Teleport stashed under a Drive piggyback (the Chrono
/// Miner's) runs nothing, so a warp it armed waits for End_Piggyback.
pub(crate) fn teleport_process_active(entity: &crate::sim::game_entity::GameEntity) -> bool {
    entity
        .locomotor
        .as_ref()
        .is_some_and(|locomotor| locomotor.active_kind() == LocomotorKind::Teleport)
}

impl crate::sim::world::Simulation {
    /// Foot Limbo's first Limbo calls ILocomotion `+0x9C(0)` (`0x004DB324`).
    /// Teleport's (`0x0071A090`) calls the owner's vtable `+0xF4` at
    /// Head_To_Coord, which is the owner's Location (`0x0055ACA0`). For an
    /// infantryman that is what clears his sub-cell: Infantry Mark clears none
    /// (`0x0047EAFE`).
    pub(crate) fn release_teleport_occupation_before_foot_limbo(&mut self, id: u64) {
        let Some(coord) = self.substrate.entities.get(id).and_then(|entity| {
            (!entity.lifecycle.in_limbo && teleport_process_active(entity))
                .then(|| super::ground_pose::position_world_coord(&entity.position))
        }) else {
            return;
        };
        self.object_raw_receiver_at(id, coord, false);
    }
}

/// Process `0x00719375..0x007193C1`: an owner whose exact coordinate is
/// already the armed destination takes `vt+0x480(NULL, 1)` and Stop_Moving
/// instead of the warp (`0x007197AF`): no animation, sound or PerCell.
pub(crate) fn warp_destination_reached(
    entity: &crate::sim::game_entity::GameEntity,
    _terrain: Option<&crate::map::resolved_terrain::ResolvedTerrainGrid>,
) -> bool {
    entity
        .locomotor
        .as_ref()
        .and_then(|l| l.teleport_runtime())
        .and_then(|runtime| runtime.warp())
        .filter(|state| state.phase == TeleportPhase::Relocate)
        .and_then(|state| state.destination)
        .is_some_and(|destination| {
            super::ground_pose::position_world_coord(&entity.position) == destination
        })
}

/// Arm the warp: the destination request (`0x007181DB`) and its chrono delay,
/// from the Euclidean lepton distance (`compute_chrono_delay`). A harvester
/// (the Chrono Miner) takes no delay, so its Relocate finishes in one frame.
fn arm_teleport(
    entity: &mut crate::sim::game_entity::GameEntity,
    destination: DriveCoord,
    target: (u16, u16),
    rules: &GeneralRules,
    is_harvester: bool,
) -> bool {
    // Compute distance in leptons (1 cell = 256 leptons) for chrono delay.
    let dx = (entity.position.rx as i32 - target.0 as i32) * 256;
    let dy = (entity.position.ry as i32 - target.1 as i32) * 256;
    let dist_sq = (dx as i64) * (dx as i64) + (dy as i64) * (dy as i64);
    let distance_leptons = isqrt_i64(dist_sq) as i32;
    let chrono_ticks = if is_harvester {
        0
    } else {
        compute_chrono_delay(rules, distance_leptons)
    };

    // Remove any existing ground movement.
    entity.movement_target = None;

    // Attach the teleport state machine — starts in Relocate (instant).
    let teleport_state = TeleportState {
        phase: TeleportPhase::Relocate,
        destination: Some(destination),
        being_warped_ticks: chrono_ticks,
    };
    let Some(runtime) = entity
        .locomotor
        .as_mut()
        .and_then(|l| l.teleport_runtime_mut())
    else {
        return false;
    };
    runtime.resolved = Some(destination);
    runtime.requested = true;
    runtime.warp = Some(teleport_state);
    entity.push_debug_event(
        0,
        DebugEventKind::SpecialMovementStart {
            kind: "Teleport".into(),
        },
    );

    true
}

/// Drop attack locks that name `teleporting_id`, the warp's first step: the
/// analogue of its Techno detach sweep (`0x0070D4A0`, called at
/// `0x007193C7`). Radio and presentation links remain root-owned integration
/// work.
///
/// RESIDUAL: the bullet sweep after it (`0x007193CC..0x007193F4`) retargets
/// every live bullet aimed at the owner (+0x10C) to the cell the owner leaves
/// (`0x00468430`; NULL for an airborne owner). VERA's projectiles keep homing
/// on the owner. Trigger: a shot in flight at a Chrono Miner, Legionnaire,
/// Commando or Ivan the frame it warps. Effect: the shot follows it to the
/// destination instead of landing on the old cell.
pub(crate) fn release_incoming_target_locks(entities: &mut EntityStore, teleporting_id: u64) {
    for id in entities.keys_sorted() {
        if id == teleporting_id {
            continue;
        }
        let Some(entity) = entities.get_mut(id) else {
            continue;
        };
        if entity.attack_target.as_ref().is_some_and(|target| {
            matches!(target.target, crate::sim::combat::TargetKind::Entity(id) if id == teleporting_id)
        }) {
            entity.attack_target = None;
        }
    }
}

/// Teleport Process (`0x007192F0`)'s own state for one owner, which the
/// object turn admits with Teleport as its active locomotor: Relocate moves
/// the owner in one frame (`0x00719631..0x007196B2`); ChronoDelay then counts
/// `being_warped_ticks` down each frame until the teleport completes. `None`
/// when the owner has no warp armed or warping in.
pub fn process_teleport(
    entities: &mut EntityStore,
    id: u64,
    sim_tick: u64,
    terrain: Option<&crate::map::resolved_terrain::ResolvedTerrainGrid>,
) -> Option<SpecialMovementOutcome> {
    let reached = entities
        .get(id)
        .is_some_and(|entity| warp_destination_reached(entity, terrain));
    let entity = entities.get_mut(id)?;
    // Temporarily take the same owned warp while committing Object coordinates.
    // The persistent resolver coordinate remains in the complete payload.
    let mut teleport = entity
        .locomotor
        .as_mut()?
        .teleport_runtime_mut()?
        .warp
        .take()?;
    let mut finished = false;
    let outcome;

    // Track phase before processing to detect transitions.
    let phase_before = teleport.phase;

    match teleport.phase {
        // 0x007197AF: Stop_Moving only; the caller runs the NULL assign.
        TeleportPhase::Relocate if reached => {
            finished = true;
            outcome = SpecialMovementOutcome::Abort;
        }
        TeleportPhase::Relocate => {
            // Instant relocation in one frame.
            let destination = teleport
                .destination
                .expect("armed Teleport has destination XYZ");
            super::ground_pose::set_position_world_xy(
                &mut entity.position,
                [destination.x, destination.y],
            );
            // Process719631..7196B2: SetCoords, resolve destination bridge,
            // then Object+1CC/5F5FA0 SetHeight(0). An old split altitude
            // must not reappear in the arrival XYZ.
            // RESIDUAL: native sets the Location through
            // FootClass::SetLocation (vt+0x1B4, 0x00719637 and again at
            // 0x00719684); the retained Process adapter applies SetHeight(0)
            // after its XY write and skips the OpenTopped rider tail. Trigger:
            // a loaded OpenTopped transport (retail: the Drive BFRT) warped
            // by a superweapon, whose SuperClass code gives any Foot a
            // Teleport locomotor (0x006CC989..0x006CC999); VERA ports no such
            // warp yet. Effect: the riders stay at the departure point.
            if let Some(terrain) = terrain {
                let cell = terrain.native_cell_identity((
                    (destination.x / 256) as i16,
                    (destination.y / 256) as i16,
                ));
                entity.on_bridge = terrain.native_cell_flags(cell) & 0x100 != 0;
            }
            super::ground_pose::set_height(
                &mut entity.position,
                entity.on_bridge,
                0,
                terrain,
                None,
            );
            // The path layer follows the destination's OnBridge. The cell
            // lists move through the caller's Mark pair around this
            // relocation (`0x007195D4` UP, `0x007196B8` DOWN).
            if let Some(locomotor) = entity.locomotor.as_mut() {
                locomotor.layer = if entity.on_bridge {
                    crate::sim::movement::locomotor::MovementLayer::Bridge
                } else {
                    crate::sim::movement::locomotor::MovementLayer::Ground
                };
            }
            // Harvester instant-warp: when chrono delay is 0, finish in one
            // frame (cleanup runs at end of this frame) — no post-warp lock.
            if teleport.being_warped_ticks == 0 {
                finished = true;
                outcome = SpecialMovementOutcome::Complete;
            } else {
                teleport.phase = TeleportPhase::ChronoDelay;
                outcome = SpecialMovementOutcome::Continue;
            }
        }
        TeleportPhase::ChronoDelay => {
            // Count down chrono delay frames. Unit remains 50% translucent until 0.
            if teleport.being_warped_ticks > 0 {
                teleport.being_warped_ticks -= 1;
            }
            if teleport.being_warped_ticks == 0 {
                finished = true;
                outcome = SpecialMovementOutcome::Complete;
            } else {
                outcome = SpecialMovementOutcome::Continue;
            }
        }
    }

    // Log phase transition if it changed.
    let phase_after = teleport.phase;
    if phase_after != phase_before {
        let phase_name = format!("{:?}", phase_after);
        entity.push_debug_event(
            sim_tick as u32,
            DebugEventKind::SpecialMovementPhase { phase: phase_name },
        );
    }
    if finished {
        entity.push_debug_event(sim_tick as u32, DebugEventKind::SpecialMovementEnd);
    }
    let runtime = entity.locomotor.as_mut()?.teleport_runtime_mut()?;
    if phase_before == TeleportPhase::Relocate {
        // The represented relocation's Stop/request retirement; full native
        // Process scheduling and Chronosphere remain explicitly separate.
        runtime.requested = false;
    }
    runtime.warp = (!finished).then_some(teleport);
    Some(outcome)
}

#[cfg(test)]
#[path = "teleport_cell_destination_tests.rs"]
mod cell_destination_tests;

#[cfg(test)]
mod tests {
    use super::*;

    use crate::rules::locomotor_type::LocomotorKind;
    use crate::sim::entity_store::EntityStore;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::movement::locomotor::LocomotorState;

    /// An owner whose active locomotor is Teleport.
    fn teleport_owner(id: u64, name: &str, rx: u16, ry: u16) -> GameEntity {
        let mut entity = GameEntity::test_default(id, name, "Americans", rx, ry);
        entity.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Teleport));
        entity
    }

    fn default_rules() -> GeneralRules {
        GeneralRules::default()
    }

    #[test]
    fn stop_moving_drops_an_armed_warp_but_not_a_warp_in() {
        let mut entities = EntityStore::new();
        entities.insert(teleport_owner(1, "CLEG", 5, 5));
        let rules = default_rules();
        let owner = entities.get_mut(1).unwrap();
        assert!(arm_teleport(
            owner,
            DriveCoord::cell(20, 20, 0),
            (20, 20),
            &rules,
            false
        ));
        teleport_stop_moving(owner);
        assert!(owner.teleport_state().is_none());

        assert!(arm_teleport(
            owner,
            DriveCoord::cell(20, 20, 0),
            (20, 20),
            &rules,
            false
        ));
        process_teleport(&mut entities, 1, 0, None);
        let owner = entities.get_mut(1).unwrap();
        assert!(owner.is_warping_in());
        teleport_stop_moving(owner);
        assert!(owner.is_warping_in());
    }

    #[test]
    fn test_teleport_issues_and_completes() {
        let mut entities = EntityStore::new();
        let mut e = teleport_owner(1, "CLEG", 5, 5);
        e.position.z = 0;
        entities.insert(e);
        let rules = default_rules();

        assert!(arm_teleport(
            entities.get_mut(1).unwrap(),
            DriveCoord::cell(20, 20, 0),
            (20, 20),
            &rules,
            false
        ));
        let entity = entities.get(1).expect("should exist");
        let ts = entity.teleport_state().expect("should have TeleportState");
        assert_eq!(ts.phase(), TeleportPhase::Relocate);
        assert!(
            ts.being_warped_ticks() >= 16,
            "should have at least minimum delay"
        );

        // One admitted frame relocates instantly.
        process_teleport(&mut entities, 1, 0, None);

        let entity = entities.get(1).expect("should exist");
        assert_eq!(entity.position.rx, 20, "Should have relocated to target");
        assert_eq!(entity.position.ry, 20);
        let ts = entity.teleport_state().expect("still warping");
        assert_eq!(
            ts.phase(),
            TeleportPhase::ChronoDelay,
            "should be in chrono delay"
        );

        // Advance through the ChronoDelay countdown.
        let delay = ts.being_warped_ticks();
        for _ in 0..delay + 5 {
            process_teleport(&mut entities, 1, 0, None);
        }

        // TeleportState should be removed after completion.
        let entity = entities.get(1).expect("should exist");
        assert!(
            entity.teleport_state().is_none(),
            "TeleportState should be removed after completion"
        );
    }

    #[test]
    fn test_chrono_delay_formula() {
        let mut rules = default_rules();
        // Default: factor=48, minimum=16, trigger=true, range_minimum=0

        // Short distance: 256 leptons (1 cell) → 256/48 = 5, clamped to 16
        assert_eq!(compute_chrono_delay(&rules, 256), 16);

        // Medium distance: 5120 leptons (20 cells) → 5120/48 = 106
        assert_eq!(compute_chrono_delay(&rules, 5120), 106);

        // Very short distance below range minimum
        rules.chrono_range_minimum = 512;
        assert_eq!(compute_chrono_delay(&rules, 200), 16); // forced to minimum

        // ChronoTrigger=false → always minimum
        rules.chrono_trigger = false;
        assert_eq!(compute_chrono_delay(&rules, 5120), 16);
    }

    /// Harvester units skip the chrono lock entirely — when is_harvester=true
    /// the lock duration is 0 regardless of distance.
    #[test]
    fn test_harvester_skips_chrono_delay() {
        let mut entities = EntityStore::new();
        let e = teleport_owner(1, "CMIN", 5, 5);
        entities.insert(e);
        let rules = default_rules();

        // Long distance (~80 cells diagonal) — non-harvester computes ~604 frames delay.
        assert!(arm_teleport(
            entities.get_mut(1).unwrap(),
            DriveCoord::cell(90, 90, 0),
            (90, 90),
            &rules,
            true
        ));
        let ts = entities
            .get(1)
            .and_then(|e| e.teleport_state())
            .expect("should have TeleportState");
        assert_eq!(
            ts.being_warped_ticks(),
            0,
            "harvester instant-warp must zero the chrono lock"
        );
    }

    /// With is_harvester=true, the Relocate phase finishes the teleport in a single
    /// tick (skipping ChronoDelay).
    #[test]
    fn test_harvester_relocate_cleans_up_in_one_tick() {
        let mut entities = EntityStore::new();
        entities.insert(teleport_owner(1, "CMIN", 5, 5));
        let rules = default_rules();

        assert!(arm_teleport(
            entities.get_mut(1).unwrap(),
            DriveCoord::cell(20, 20, 0),
            (20, 20),
            &rules,
            true
        ));

        // Single frame: position snaps, then cleanup runs because being_warped_ticks==0.
        process_teleport(&mut entities, 1, 0, None);

        let entity = entities.get(1).expect("should exist");
        assert_eq!(entity.position.rx, 20);
        assert_eq!(entity.position.ry, 20);
        assert!(
            entity.teleport_state().is_none(),
            "harvester teleport should clean up in one frame"
        );
    }

    /// Regression: non-harvester (Chrono Legionnaire path) still goes through the
    /// full Relocate → ChronoDelay countdown.
    #[test]
    fn test_non_harvester_uses_full_chrono_delay() {
        let mut entities = EntityStore::new();
        let e = teleport_owner(1, "CLEG", 5, 5);
        entities.insert(e);
        let rules = default_rules();

        assert!(arm_teleport(
            entities.get_mut(1).unwrap(),
            DriveCoord::cell(20, 20, 0),
            (20, 20),
            &rules,
            false
        ));
        let initial_ticks = entities
            .get(1)
            .and_then(|e| e.teleport_state())
            .map(|t| t.being_warped_ticks())
            .expect("teleport_state");
        assert!(
            initial_ticks > 0,
            "non-harvester must keep the distance-based chrono lock"
        );

        // Frame 1: Relocate snaps position and transitions to ChronoDelay (NOT cleanup).
        process_teleport(&mut entities, 1, 0, None);
        let ts = entities
            .get(1)
            .and_then(|e| e.teleport_state())
            .expect("still warping after Relocate");
        assert_eq!(ts.phase(), TeleportPhase::ChronoDelay);
        assert_eq!(ts.being_warped_ticks(), initial_ticks);
    }

    #[test]
    fn teleport_exposes_distinct_warp_and_targetability_producers() {
        let relocate = TeleportState::for_test(TeleportPhase::Relocate, 1, 1, 10);
        assert!(!relocate.warp_in_active());
        assert!(!relocate.is_targetable());

        let arrival = TeleportState::for_test(TeleportPhase::ChronoDelay, 1, 1, 10);
        assert!(arrival.warp_in_active());
        assert!(arrival.is_targetable());
    }
}
