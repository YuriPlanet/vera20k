//! `FlyLocomotionClass::Process @ 0x004CCB40`, the Fly locomotor's frame for
//! the object it flies, with the functions only it calls:
//! UpdateFlightMotion (`0x004CD600`), Horizontal_Step (`0x004CEFB0`) and the
//! playfield latch (`0x004CD510`). The vertical step, the target speed and
//! its ramp keep their owners (`movement::fly_height`,
//! `movement::air_movement`), as do the phase transitions
//! (`Simulation::complete_fly_phase`) and the dead fall
//! (`Simulation::fly_crash_fall`).
//!
//! Evidence: tools/spatial_oracle/fly_process.py runs Process frame by frame
//! for 25 living aircraft (cruise, turns, slowdown and landing at a ground
//! cell, armed circling, strafe, the release latch, HunterSeeker, FlyBy,
//! Guard to Move, Commence, the flight level's restore and selection, the
//! climb, the landing drift, IsDropship, a dock in radio contact, the attack
//! cell of a dock out of contact, the AirportBound refusal);
//! `fly_process_tests` replays every frame through [`Simulation::fly_process`].
//! tools/spatial_oracle/fly_map_edge.py runs the map-edge admission, which
//! [`fly_map_edge`] replays; aircraft_crash.py the dead fall.
//!
//! RESIDUAL: the spin (`0x004CE5A0`). Power_Off (`0x004CFD20`) starts it on
//! a moving Fly, with two Scenario draws, and then the unpowered fall
//! (`Simulation::fly_crash_fall`) drops it. Nothing in VERA powers off an
//! aircraft (native: EMP), so both are dormant.
//!
//! RESIDUAL: Horizontal_Step's deselect (`0x004CF55D..0x004CF5EC`): a
//! selected aircraft of a house no human controls is deselected (vt+0x150)
//! over shroud, or fog while fog is on. Selection is the client's.

use super::Simulation;
use crate::map::entities::EntityCategory;
use crate::rules::object_type::ObjectType;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::TargetKind;
use crate::sim::components::{DriveCoord, NavTargetRef};
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::movement::air_movement::{self, AirMovementTickStats};
use crate::sim::movement::fly_height::{ARRIVAL_STOP, ARRIVAL_THREE_QUARTERS};
// The cell of a coordinate as `0x0041BEA0` (vt+0x1B8) and the Fly's cell
// tests form it.
use crate::sim::movement::coord_cell;
use crate::sim::movement::ground_pose;
use crate::sim::rng::SimRng;
use crate::sim::world::FrameEffects;
use crate::util::fixed_math::SIM_ZERO;

#[cfg(test)]
#[path = "fly_process_tests.rs"]
mod tests;

/// `CoordStruct` empty (`0x008B3C78`), Fly's "no destination".
const NULL_COORD: DriveCoord = DriveCoord { x: 0, y: 0, z: 0 };

/// Horizontal_Step's Secondary turns to land or to the Target within this
/// distance (`0x004CF29A`).
const ARRIVAL_FACING_DISTANCE: i32 = 0x100;

/// A strafer this close to its destination (`0x004CF20F`) keeps its heading
/// unless it is within this angle of the destination's (`0x004CF21A`).
const STRAFE_HOLD_DISTANCE: i32 = 0x300;
const STRAFE_HOLD_ANGLE: i32 = 0x2000;

/// The landing drift's step toward the destination cell's centre
/// (`0x007E3568`, 5.0).
const LANDING_DRIFT_STEP: i32 = 5;

/// The map-edge push back toward the map (`0x004CDB8F`) and the scatter's
/// radius (`0x004CDC99`).
const MAP_EDGE_PUSH: i32 = 0x80;
const MAP_EDGE_SCATTER: i32 = 0x40;

/// What the map-edge admission reads of the map.
#[derive(Debug, Clone, Copy)]
pub(crate) struct FlyMapEdge {
    /// `MapClass+0xF4`, `+0xF8`: the `Size=` diamond `In_Bounds` tests.
    width: i32,
    height: i32,
    /// The LocalSize `0x00565660` measures from.
    bounds: crate::sim::cell_rect::PlayfieldBounds,
    /// `MapClass+0x12C`, MapRect's width: `Resize @ 0x00565C10` writes
    /// `Size.W + Size.H - 1`.
    span: i32,
}

/// The map-edge admission of Fly's paid step (`0x004CDB4C..0x004CDD07`): a
/// candidate whose cell is in the `Size=` diamond (`MapClass::In_Bounds @
/// 0x00568300`) is placed. Outside it, it is placed as it is when the owner
/// may not leave the map (vt+0x4DC, `may_leave`; native asks it only here,
/// VERA's answer has no side effects) or is a FlyBy Aircraft (AircraftType `+0xE0B`). Otherwise its X moves 0x80 toward
/// the map's middle column (the cell's LocalSize X, `0x00565660`, against
/// half of `MapClass+0x12C`) and that is placed when in the diamond; else a
/// scatter of 0x40 (`0x0049F420`, one Scenario draw) is placed when in the
/// diamond, and nothing when not. Answers the last candidate and whether
/// SetLocation takes it.
pub(crate) fn fly_map_edge(
    candidate: DriveCoord,
    edge: &FlyMapEdge,
    may_leave: bool,
    fly_by_aircraft: bool,
    rng: &mut SimRng,
) -> (DriveCoord, bool) {
    let in_bounds = |coord: DriveCoord| {
        crate::map::playfield::size_diamond_contains(edge.width, edge.height, coord_cell(coord))
    };
    if in_bounds(candidate) || !may_leave || fly_by_aircraft {
        return (candidate, true);
    }
    let (local_x, _) =
        crate::map::playfield::cell_to_local_packed(edge.bounds, coord_cell(candidate));
    let pushed = DriveCoord {
        x: if i32::from(local_x) >= edge.span / 2 {
            candidate.x.wrapping_sub(MAP_EDGE_PUSH)
        } else {
            candidate.x.wrapping_add(MAP_EDGE_PUSH)
        },
        ..candidate
    };
    if in_bounds(pushed) {
        return (pushed, true);
    }
    let (x, y) = crate::sim::combat::inviso_scatter::random_direction_coord(
        rng,
        pushed.x,
        pushed.y,
        MAP_EDGE_SCATTER,
        crate::sim::combat::inviso_scatter::RandomDirectionSnap::Preserve,
    );
    let scattered = DriveCoord { x, y, ..pushed };
    (scattered, in_bounds(scattered))
}

impl Simulation {
    /// The type's FlightLevel of live object `id`
    /// ([`air_movement::type_flight_level`]).
    fn fly_type_flight_level(&self, id: u64, rules: Option<&RuleSet>) -> i32 {
        let entity = self.substrate.entities.get(id).expect("Fly owner");
        air_movement::type_flight_level(entity, rules.map(|rules| (rules, &self.interner)))
    }

    /// One Process visit of a Fly object on the map:
    /// - the type's FlightLevel restored (`0x004CCB47..0x004CCB81`,
    ///   `FlyRuntime::restore_flight_level`);
    /// - the queued mission commenced when ready, unless it lands or takes
    ///   off (`0x004CCB84..0x004CCBAD`: vt+0x200, vt+0x1EC);
    /// - UpdateFlightMotion ([`Self::fly_update_flight_motion`]), whose dead
    ///   fall may end in the impact the object turn commits;
    /// - nothing more unpowered or no longer alive (`0x004CCBC4..0x004CCBE6`);
    /// - Horizontal_Step toward the destination while it lives, holds a
    ///   destination, neither lands nor takes off and is above the ground
    ///   (`0x004CCBE9..0x004CCC49`);
    /// - the phase transitions while it lives (`0x004CD2A0`,
    ///   [`Self::complete_fly_phase`]);
    /// - the playfield latch ([`Self::fly_playfield_latch`]).
    ///
    /// Native answers Is_Moving; no caller reads it.
    pub(super) fn fly_process(
        &mut self,
        id: u64,
        rules: Option<&RuleSet>,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> AirMovementTickStats {
        let mut stats = AirMovementTickStats::default();
        let flight_level = self.fly_type_flight_level(id, rules);
        let Some(entity) = self.substrate.entities.get_mut(id) else {
            return stats;
        };
        air_movement::ensure_fly_secondary_facing(entity);
        let living = entity.health.current() > 0;
        let Some(state) = entity
            .locomotor
            .as_mut()
            .and_then(|locomotor| locomotor.fly_runtime_mut())
        else {
            return stats;
        };
        state.restore_flight_level(flight_level);
        let in_phase = state.has_phase_callback();
        if living
            && !in_phase
            && rules.is_some_and(|rules| self.mission_ready_to_commence(id, rules))
        {
            let _ = self.mission_commence_exact(id, self.session.binary_frame());
        }
        if self.fly_update_flight_motion(id, rules, registry, frame_effects) {
            stats.impact = true;
            return stats;
        }
        let Some(entity) = self.substrate.entities.get(id) else {
            return stats;
        };
        let Some(state) = entity
            .locomotor
            .as_ref()
            .filter(|locomotor| locomotor.is_powered())
            .and_then(|locomotor| locomotor.fly_runtime())
        else {
            return stats;
        };
        if !entity.lifecycle.object_alive() {
            return stats;
        }
        let destination = state.destination();
        if entity.health.current() > 0
            && destination != NULL_COORD
            && !state.has_phase_callback()
            && air_movement::current_fly_height(entity, self.resolved_terrain.as_ref()) > 0
        {
            self.fly_horizontal_step(id, destination, true, rules, registry, frame_effects);
        }
        if self
            .substrate
            .entities
            .get(id)
            .is_some_and(|entity| entity.health.current() > 0)
        {
            self.complete_fly_phase(id, rules, registry, frame_effects);
        }
        self.fly_playfield_latch(id, rules, registry, frame_effects);
        stats
    }

    /// `0x004CD600`, the flight itself:
    /// - the AircraftTracker follows a cell change while the current speed is
    ///   not zero (`0x004CD60E..0x004CD65F`: vt+0x1B8 against `+0x560`,
    ///   Is_Moving_Now, `0x004138C0`);
    /// - Enter drops cruise mode (`0x004CD664..0x004CD67B`);
    /// - a dead Fly above the ground falls ([`Self::fly_crash_fall`]); its
    ///   impact returns true;
    /// - a living one guarding with a NavCom and nothing queued, not
    ///   AirportBound (Fly `+0x18`), queues Move (`0x004CD9C8..0x004CDA02`);
    /// - past Is_Moving (`0x004CDA08`, `0x004CCA90`) the step
    ///   ([`Self::fly_flight_step`]).
    fn fly_update_flight_motion(
        &mut self,
        id: u64,
        rules: Option<&RuleSet>,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> bool {
        let Some(entity) = self.substrate.entities.get(id) else {
            return false;
        };
        let cell = coord_cell(ground_pose::object_location(
            entity,
            self.resolved_terrain.as_ref(),
        ));
        let moving_now = entity
            .locomotor
            .as_ref()
            .and_then(|locomotor| locomotor.fly_runtime())
            .is_some_and(crate::sim::movement::fly_height::FlyRuntime::is_moving_now);
        if cell != entity.air_tracker_cell() && moving_now {
            self.aircraft_tracker_update_cell(id, cell);
        }
        let entity = self.substrate.entities.get_mut(id).unwrap();
        let mission = entity.mission.effective();
        if let Some(state) = entity
            .locomotor
            .as_mut()
            .and_then(|locomotor| locomotor.fly_runtime_mut())
        {
            state.prepare_process(mission);
        }
        if self.fly_crash_fall(id, rules, registry, frame_effects) {
            return true;
        }
        self.fly_guard_to_move(id);
        let Some(entity) = self.substrate.entities.get(id) else {
            return false;
        };
        if crate::sim::movement::motion_query::is_moving(entity) != Some(true) {
            return false;
        }
        self.fly_flight_step(id, rules, registry, frame_effects);
        false
    }

    /// `0x004CD9C8..0x004CDA02`: `Queue_Mission(Move, 0)` (vt+0x1E8) for a
    /// living owner with a NavCom (`+0x5A4`) whose mission (vt+0x184) is
    /// Guard with nothing queued (`+0xB4`), unless the Fly is AirportBound.
    fn fly_guard_to_move(&mut self, id: u64) {
        let Some(entity) = self.substrate.entities.get_mut(id) else {
            return;
        };
        let airport_bound = entity
            .locomotor
            .as_ref()
            .and_then(|locomotor| locomotor.fly_runtime())
            .is_none_or(|state| state.airport_bound());
        if entity.health.current() <= 0
            || entity.navigation.nav_com().is_none()
            || entity.mission.effective() != MissionId::from_known(MissionType::Guard)
            || entity.mission.queued() != MissionId::NONE
            || airport_bound
        {
            return;
        }
        crate::sim::mission::authority::queue_entity_mission_deferred(
            entity,
            MissionId::from_known(MissionType::Move),
        );
    }

    /// UpdateFlightMotion past its Is_Moving gate (`0x004CDA16..0x004CE4A2`),
    /// between the owner's Mark(UP) (`0x004CDA36`) and Mark(DOWN)
    /// (`0x004CE49C`):
    /// - the paid step: the type's Speed times the current speed along
    ///   Primary.Current from the Location (`0x004CDA3C..0x004CDB47`,
    ///   interface `+0x84` = `0x004CFE20`), through the map-edge admission
    ///   ([`fly_map_edge`]) to `FootClass::SetLocation` (vt+0x1B4);
    /// - the distance to the destination from the placed Location
    ///   (`0x004CDD75..0x004CDDD3`);
    /// - the height step (`FlyRuntime::step_height`) and, after a descent,
    ///   the landing drift ([`Self::fly_landing_drift`]);
    /// - the target speed (`FlyRuntime::write_target_speed`), the
    ///   IsDropship approach attitude (`0x004CE2E5..0x004CE3BA`), the landing
    ///   trigger ([`Self::fly_landing_trigger`]) and the speed ramp
    ///   (`0x004CE441..0x004CE495`).
    fn fly_flight_step(
        &mut self,
        id: u64,
        rules: Option<&RuleSet>,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) {
        let frame = self.session.binary_frame();
        self.foot_mark_remove(id, rules, registry, frame_effects);
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        let object = rules.and_then(|rules| self.interner.object_type(entity.type_ref(), rules));
        let dropship = object.is_some_and(|object| object.is_dropship);
        let location = ground_pose::object_location(entity, self.resolved_terrain.as_ref());
        let speed = air_movement::current_fly_speed(
            object.map_or(0, |object| {
                crate::util::fixed_math::ra2_speed_to_leptons_per_frame(object.speed)
            }),
            entity
                .locomotor
                .as_ref()
                .and_then(|locomotor| locomotor.fly_runtime())
                .map_or(SIM_ZERO, |state| state.current_speed()),
        );
        let mut candidate = location;
        if speed > 0 {
            let [x, y] = crate::util::native_trig::facing_step_world_xy(
                [location.x, location.y],
                entity.body_facing.current(frame),
                speed,
            );
            candidate = DriveCoord { x, y, ..location };
        }
        if let Some(coord) = self.fly_map_edge_admission(id, candidate, object) {
            ground_pose::foot_set_location(
                &mut self.substrate.entities,
                id,
                coord,
                rules,
                &self.interner,
            );
        }
        let terrain = self.resolved_terrain.as_ref();
        let entity = self.substrate.entities.get_mut(id).unwrap();
        let placed = ground_pose::object_location(entity, terrain);
        let destination = entity
            .locomotor
            .as_ref()
            .and_then(|locomotor| locomotor.fly_runtime())
            .map_or(NULL_COORD, |state| state.destination());
        let distance = crate::sim::cell_kernel::native_xy_distance(
            destination.x.wrapping_sub(placed.x),
            destination.y.wrapping_sub(placed.y),
        );
        let height = air_movement::update_fly_height(
            entity,
            terrain,
            rules.map(|rules| (rules, &self.interner)),
        );
        if height.descended() {
            self.fly_landing_drift(id, rules);
        }
        let terrain = self.resolved_terrain.as_ref();
        let rules_context = rules.map(|rules| (rules, &self.interner));
        let entity = self.substrate.entities.get_mut(id).unwrap();
        let facts = air_movement::fly_speed_facts(entity, distance, terrain, rules_context);
        if let Some(state) = entity
            .locomotor
            .as_mut()
            .and_then(|locomotor| locomotor.fly_runtime_mut())
        {
            state.write_target_speed(&facts);
        }
        if let Some(object) = object.filter(|_| dropship)
            && entity.health.current() > 0
            && entity
                .locomotor
                .as_ref()
                .and_then(|locomotor| locomotor.fly_runtime())
                .is_some_and(|state| state.current_speed() > SIM_ZERO && !state.taking_off())
        {
            entity
                .flight_attitude
                .approach(distance, object.slowdown_distance, object.pitch_angle);
        }
        if self.fly_landing_trigger(id) {
            self.begin_fly_landing(id, rules, registry, frame_effects);
        }
        if let Some(entity) = self.substrate.entities.get_mut(id)
            && entity.health.current() > 0
            && let Some(state) = entity
                .locomotor
                .as_mut()
                .and_then(|locomotor| locomotor.fly_runtime_mut())
        {
            state.ramp_speed();
        }
        // Mark refuses an object in Limbo (`0x005F5854`).
        if self
            .substrate
            .entities
            .get(id)
            .is_some_and(|entity| entity.lifecycle.object_alive() && !entity.lifecycle.in_limbo())
        {
            self.foot_mark_put(id, rules, registry, frame_effects);
        }
    }

    /// [`fly_map_edge`] for the owner: its map (`MapClass+0x12C` is
    /// [`Self::map_rect`]'s width), vt+0x4DC ([`Self::aircraft_may_leave_map`]),
    /// its FlyBy type and the Scenario RNG; the coordinate SetLocation takes.
    /// Without MapClass authority (headless fixtures) the candidate is placed
    /// as it is.
    ///
    /// RESIDUAL: a Fly owner that is not an Aircraft asks its own vt+0x4DC;
    /// VERA reads it as false (no retail type has one).
    fn fly_map_edge_admission(
        &mut self,
        id: u64,
        candidate: DriveCoord,
        object: Option<&ObjectType>,
    ) -> Option<DriveCoord> {
        let (Some((width, height)), Some(bounds), Some([_, _, span, _])) = (
            self.map_size_diamond(),
            self.playfield_bounds,
            self.map_rect(),
        ) else {
            return Some(candidate);
        };
        let aircraft = self
            .substrate
            .entities
            .get(id)
            .is_some_and(|entity| entity.category() == EntityCategory::Aircraft);
        let edge = FlyMapEdge {
            width,
            height,
            bounds,
            span,
        };
        let may_leave = aircraft && self.aircraft_may_leave_map(id);
        let (coord, placed) = fly_map_edge(
            candidate,
            &edge,
            may_leave,
            aircraft && object.is_some_and(|object| object.fly_by),
            &mut self.scenario_rng,
        );
        placed.then_some(coord)
    }

    /// The landing drift (`0x004CDFBC..0x004CE13D`), after a descent's
    /// SetHeight: a living Fly holding a destination steps 5 leptons from its
    /// Location toward the centre of the destination's cell (facing
    /// `0x004CAE30`, step as the paid step's) through `FootClass::SetLocation`,
    /// unless it enters (vt+0x184 Enter) a Helipad building its NavCom names.
    fn fly_landing_drift(&mut self, id: u64, rules: Option<&RuleSet>) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        let destination = entity
            .locomotor
            .as_ref()
            .and_then(|locomotor| locomotor.fly_runtime())
            .map_or(NULL_COORD, |state| state.destination());
        if entity.health.current() <= 0 || destination == NULL_COORD {
            return;
        }
        let entering_helipad = entity.mission.effective()
            == MissionId::from_known(MissionType::Enter)
            && entity
                .navigation
                .nav_com()
                .and_then(|nav_com| match nav_com {
                    NavTargetRef::Cell { .. } => None,
                    NavTargetRef::Object { id } => self.substrate.entities.get(id),
                })
                .filter(|building| building.category() == EntityCategory::Structure)
                .and_then(|building| {
                    rules.and_then(|rules| self.interner.object_type(building.type_ref(), rules))
                })
                .is_some_and(|object| object.helipad);
        if entering_helipad {
            return;
        }
        let location = ground_pose::object_location(entity, self.resolved_terrain.as_ref());
        let (cell_x, cell_y) = coord_cell(destination);
        let centre = [
            i32::from(cell_x).wrapping_mul(256).wrapping_add(0x80),
            i32::from(cell_y).wrapping_mul(256).wrapping_add(0x80),
        ];
        let facing =
            crate::util::direction_tables::facing16_between([location.x, location.y], centre);
        let [x, y] = crate::util::native_trig::facing_step_world_xy(
            [location.x, location.y],
            facing,
            LANDING_DRIFT_STEP,
        );
        ground_pose::foot_set_location(
            &mut self.substrate.entities,
            id,
            DriveCoord { x, y, ..location },
            rules,
            &self.interner,
        );
    }

    /// The landing trigger (`0x004CE3C0..0x004CE43C`): Begin_Landing for a
    /// living Fly out of cruise mode, neither landing nor taking off, over its
    /// destination's cell with a zero target speed and no Target or no Ammo.
    fn fly_landing_trigger(&self, id: u64) -> bool {
        let Some(entity) = self.substrate.entities.get(id) else {
            return false;
        };
        let Some(state) = entity
            .locomotor
            .as_ref()
            .and_then(|locomotor| locomotor.fly_runtime())
        else {
            return false;
        };
        let location = ground_pose::object_location(entity, self.resolved_terrain.as_ref());
        !state.cruise_mode()
            && entity.health.current() > 0
            && !state.has_phase_callback()
            && coord_cell(state.destination()) == coord_cell(location)
            && state.target_speed() == SIM_ZERO
            && (entity.attack_target.target().is_none()
                || entity.ammo().map_or(-1, |ammo| ammo.current()) == 0)
    }

    /// `FlyLocomotionClass::Horizontal_Step @ 0x004CEFB0` toward `coord` (the
    /// destination, from Process): the heading, the Secondary facing, the
    /// height target and the arrival. Answers the distance it measured.
    ///
    /// The distance is to `coord` unless a building stands in its cell
    /// (`0x0047C520`) and the owner is not HunterSeeker: then to the
    /// building's dock coordinate (vt+0xA8, `0x00447B20`) when the building
    /// is in the owner's radio contact, else for an Aircraft to the cell
    /// Find_Attack_Cell (`0x00418E20`) picks around it, or the empty
    /// coordinate when that has none. Headings aim at `coord` itself.
    ///
    /// `may_slow` (Process passes 1) is dropped for an Aircraft with a
    /// Target, Ammo and a strafing weapon (IFlyControl `+0x18`) and for a
    /// FlyBy Aircraft.
    /// - Primary (`0x004CF186..0x004CF28F`): toward `coord` (`0x004265B0`)
    ///   unless the Aircraft's release latch holds (IFlyControl `+0x20`); a
    ///   strafer within 0x300 of `coord` keeps its heading unless already
    ///   within 0x2000 of it (`0x004D03D0`).
    /// - Secondary (`0x004CF29A..0x004CF3CB`): within 0x100 with `may_slow`
    ///   and not HunterSeeker, toward the Target (DirectionToTarget
    ///   `0x005F3DB0`) when it has one and Ammo and does not strafe, else to
    ///   the landing direction (IFlyControl `+0x10` shifted by 13, 0 for a
    ///   non-Aircraft); farther, toward `coord`. A latched Aircraft keeps it.
    /// - The height target (`0x004CF3D0..0x004CF4CF`): in cruise mode within
    ///   0x300, the destination's height above its ground; for IsDropship
    ///   within SlowdownDistance, a third of FlightLevel (the original's
    ///   blend takes an integer ratio, always 0 there); else FlightLevel.
    /// - The arrival (`0x004CF4D2..0x004CF55D`), for a non-HunterSeeker that
    ///   `0x004D0180` lets slow: within 0x80 a zero target speed (with
    ///   `may_slow`) and, out of cruise mode under 0.05 current speed,
    ///   Begin_Landing; within 0x200 half speed, within 0x300 three quarters
    ///   (with `may_slow`).
    fn fly_horizontal_step(
        &mut self,
        id: u64,
        coord: DriveCoord,
        may_slow: bool,
        rules: Option<&RuleSet>,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> i32 {
        let frame = self.session.binary_frame();
        let Some(entity) = self.substrate.entities.get(id) else {
            return 0;
        };
        let object = rules.and_then(|rules| self.interner.object_type(entity.type_ref(), rules));
        let aircraft = entity.category() == EntityCategory::Aircraft;
        let strafe = aircraft
            && rules.zip(object).is_some_and(|(rules, object)| {
                crate::sim::combat::combat_weapon::aircraft_strafes(
                    rules,
                    object,
                    entity.veterancy.rank(),
                )
            });
        let locked = aircraft
            && entity
                .mission_leaf
                .as_aircraft()
                .is_some_and(|leaf| leaf.action_latch() != 0);
        let target = entity.attack_target.target();
        let ammo = entity.ammo().map_or(-1, |ammo| ammo.current());
        let hunter_seeker = object.is_some_and(|object| object.hunter_seeker);
        let armed = target.is_some() && ammo != 0;
        let may_slow = may_slow
            && !(aircraft && armed && strafe)
            && !(aircraft && object.is_some_and(|object| object.fly_by));

        let mut aim = coord;
        if !hunter_seeker && let Some(building) = self.fly_building_at(coord) {
            if entity.radio_contacts.contains(building) {
                if let Some(dock) = rules.and_then(|rules| {
                    crate::sim::movement::building_dock_coordinate(
                        &self.substrate.entities,
                        building,
                        Some(id),
                        self.resolved_terrain.as_ref(),
                        rules,
                        &self.interner,
                    )
                }) {
                    aim = dock;
                }
            } else if aircraft && let Some(rules) = rules {
                aim = self
                    .aircraft_find_attack_cell(id, Some(NavTargetRef::object(building)), rules)
                    .and_then(|cell| {
                        let cells = self
                            .resolved_terrain
                            .as_ref()
                            .map(crate::map::resolved_terrain::NativeCellQuery::canonical);
                        ground_pose::target_get_coords(TargetKind::from(cell), self, cells.as_ref())
                    })
                    .unwrap_or(NULL_COORD);
            }
        }
        let entity = self.substrate.entities.get(id).unwrap();
        let terrain = self.resolved_terrain.as_ref();
        let owner = ground_pose::object_get_coords(entity, terrain);
        let distance = crate::sim::cell_kernel::native_xy_distance(
            owner.x.wrapping_sub(aim.x),
            owner.y.wrapping_sub(aim.y),
        );

        let toward =
            crate::util::direction_tables::facing16_between([owner.x, owner.y], [coord.x, coord.y]);
        let primary = (!locked
            && !(strafe
                && crate::sim::cell_kernel::native_xy_distance(
                    owner.x.wrapping_sub(coord.x),
                    owner.y.wrapping_sub(coord.y),
                ) <= STRAFE_HOLD_DISTANCE
                && i32::from(toward.wrapping_sub(entity.body_facing.current(frame)) as i16).abs()
                    > STRAFE_HOLD_ANGLE))
            .then_some(toward);

        let secondary = if distance < ARRIVAL_FACING_DISTANCE && may_slow && !hunter_seeker {
            match target.filter(|_| armed && !(aircraft && strafe)) {
                Some(target) => (!locked).then(|| {
                    let cells =
                        terrain.map(crate::map::resolved_terrain::NativeCellQuery::canonical);
                    let at = ground_pose::target_get_coords(target, self, cells.as_ref())
                        .unwrap_or(owner);
                    crate::util::direction_tables::facing16_between(
                        [owner.x, owner.y],
                        [at.x, at.y],
                    )
                }),
                None => {
                    let direction = if aircraft {
                        crate::sim::aircraft::landing_base::landing_direction(
                            entity,
                            &self.substrate.entities,
                            frame,
                            rules.map_or(0, |rules| rules.general.pose_dir),
                        )
                        .wrapping_shl(13)
                    } else {
                        0
                    };
                    (!locked).then_some(direction as u16)
                }
            }
        } else {
            (!locked).then_some(toward)
        };

        let flight_level = self.fly_type_flight_level(id, rules);
        let entity = self.substrate.entities.get(id).unwrap();
        let state = entity
            .locomotor
            .as_ref()
            .and_then(|locomotor| locomotor.fly_runtime())
            .expect("Horizontal_Step owner flies");
        let cruise = state.cruise_mode();
        let level = if cruise && distance < ARRIVAL_THREE_QUARTERS {
            let destination = state.destination();
            let ground = ground_pose::ground_surface_z_at(
                [destination.x, destination.y],
                false,
                self.resolved_terrain.as_ref(),
                None,
            )
            .unwrap_or(0);
            destination.z.wrapping_sub(ground)
        } else if object
            .is_some_and(|object| object.is_dropship && distance < object.slowdown_distance)
        {
            // `0x004CF432..0x004CF4B6`: f32(FL / 3) * (1 - q) + FL * q with
            // the integer quotient q = distance / SlowdownDistance, 0 here.
            // The f32 store is exact below 3 * 2^24.
            flight_level / 3
        } else {
            flight_level
        };
        let slow = air_movement::fly_speed_facts(
            entity,
            distance,
            self.resolved_terrain.as_ref(),
            rules.map(|rules| (rules, &self.interner)),
        )
        .slow;
        let arrival = !hunter_seeker && air_movement::fly_may_slow(state.landing(), cruise, &slow);
        let land = arrival
            && distance < ARRIVAL_STOP
            && !cruise
            && state.current_speed() < air_movement::MIN_CREEP_SPEED;

        let entity = self.substrate.entities.get_mut(id).unwrap();
        if let Some(primary) = primary {
            entity.body_facing.set(primary, frame);
        }
        if let Some(secondary) = secondary {
            air_movement::ensure_fly_secondary_facing(entity);
            if let Some(facing) = entity.barrel_facing.as_mut() {
                facing.set(secondary, frame);
            }
        }
        let state = entity
            .locomotor
            .as_mut()
            .and_then(|locomotor| locomotor.fly_runtime_mut())
            .unwrap();
        state.set_flight_level(level);
        if arrival && may_slow {
            state.slow_for_arrival(distance);
        }
        if land {
            self.begin_fly_landing(id, rules, registry, frame_effects);
        }
        distance
    }

    /// `0x004CD510`, after the phase transitions: a Location in the playfield
    /// (`MapClass::IsCoordInPlayfield @ 0x005785F0`) sets `+0x3D5`. Outside
    /// it, a retreating owner (vt+0x184 Retreat) leaves the map: each
    /// passenger, taken from the head (`CargoClass::RemoveFirstPassenger @
    /// 0x00473430`), marks its team leaving the map (`+0x82`) and is deleted
    /// (vt+0x20); the owner's team is marked too, then Stun (vt+0x3A0,
    /// `FootClass::Stun @ 0x004D5660`) and UnInit (vt+0xF8). Without MapClass
    /// authority (headless fixtures) nothing happens.
    ///
    /// RESIDUAL: a passenger infantry of a `Civilian=` type that is no
    /// survivor (`0x00413CD0`: InfantryType `+0xEC1`, Infantry `+0x6D9`) sets
    /// its house's CiviliansEvacuated (`+0x1F9`), which VERA does not
    /// represent. Trigger: a retreating transport aircraft carrying
    /// civilians off the map, in campaigns. Effect: the trigger event that
    /// reads the flag does not fire.
    ///
    /// RESIDUAL: each passenger is deleted at once (`0x004CD5AE`); VERA
    /// UnInits it, which announces the same expiry but removes the object at
    /// the frame's end. Trigger: the same retreat with passengers. Effect:
    /// none observed, as nothing reads a cargo passenger in between.
    fn fly_playfield_latch(
        &mut self,
        id: u64,
        rules: Option<&RuleSet>,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) {
        let Some(bounds) = self.playfield_bounds else {
            return;
        };
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        let location = ground_pose::object_location(entity, self.resolved_terrain.as_ref());
        let retreat = entity.mission.effective() == MissionId::from_known(MissionType::Retreat);
        if crate::sim::cell_rect::cell_is_in_playfield_leptons(
            (location.x, location.y, location.z),
            Some(bounds),
            self.resolved_terrain.as_ref(),
        ) {
            // `0x004CD5F0`.
            self.substrate
                .entities
                .get_mut(id)
                .unwrap()
                .in_playfield
                .enter();
            return;
        }
        if !retreat {
            return;
        }
        let context = super::UninitContext::new(rules, registry).with_effects(frame_effects);
        while let Some((passenger, _)) = self
            .substrate
            .entities
            .get_mut(id)
            .and_then(|owner| owner.passenger_role.cargo_mut())
            .and_then(|cargo| cargo.unload_first())
        {
            self.team_script_vm.mark_member_team_leaving_map(passenger);
            // RemoveFirstPassenger leaves the passenger's own hold for its
            // teardown.
            if let Some(passenger) = self.substrate.entities.get_mut(passenger) {
                passenger.passenger_role.clear_carrier_relation();
            }
            self.uninit_with_context(passenger, context);
        }
        self.team_script_vm.mark_member_team_leaving_map(id);
        self.techno_death_stun(id, context);
        self.uninit_with_context(id, context);
    }
}
