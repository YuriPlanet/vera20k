//! Jumpjet locomotor instance state and the two orders that write it.
//!
//! `JumpjetRuntime` is the locomotor's own persistent block (destination,
//! moving byte, the state field at `+0x50`, flight values). Its orders are
//! `Move_To @ 0x0054B1C0` ([`JumpjetRuntime::move_to`]) and
//! `Stop_Moving @ 0x0054B4D0` ([`JumpjetRuntime::stop_moving`]), for Unit and
//! Infantry owners alike; they reach the owner and the map through
//! [`JumpjetOrderHost`], which the world answers in
//! [`Simulation::jumpjet_move_to`] and [`Simulation::jumpjet_stop_moving`].
//! The per-frame flight is the native kernel in `jumpjet_flight`, hosted by
//! `world::jumpjet_cruise`.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on sim/locomotor, sim/movement.
//! - sim/ NEVER depends on render/, ui/, sidebar/, audio/, net/.

use super::foot_path::coord_cell;
#[path = "jumpjet_flight.rs"]
pub mod jumpjet_flight;

use crate::sim::components::DriveCoord;
use crate::sim::world::Simulation;
use crate::util::fixed_math::{SIM_ZERO, SimFixed};
use crate::util::lepton::lepton_to_cell_packed;
use jumpjet_flight::{FlightOwnerKind, STATE_ASCEND, STATE_DESCEND};

/// Persistent Jumpjet instance fields: cached XYZ at +40, moving at +4C,
/// phase at +50 (interface-relative offsets are four bytes smaller).
/// Constructor54AC40 and stream54B750/54B7E0 retain these independently.
/// AirMovePhase describes the older motion adapter and is not this authority.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct JumpjetRuntime {
    destination: DriveCoord,
    moving: bool,
    phase: i32,
    /// The type block `Link_To_Object @ 0x0054AD30` copies (`+0x1C..+0x3C`).
    params: jumpjet_flight::JumpjetFlightParams,
    /// Facing, speeds, target height and bob (`+0x54..+0x8C`).
    flight: jumpjet_flight::JumpjetFlight,
    /// Locomotor `+0x90`: State 4 sets this once it has admitted a landing, so
    /// later descent frames stop re-testing the cell. Touchdown clears it.
    #[serde(default)]
    landing_latched: bool,
}

impl Default for JumpjetRuntime {
    fn default() -> Self {
        Self {
            destination: Self::NULL,
            moving: false,
            phase: 0,
            params: Default::default(),
            flight: Default::default(),
            landing_latched: false,
        }
    }
}

/// What `Move_To` made of a request.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct MoveToOutcome {
    /// A descent was lifted back into the climb (`0x0054B455..0x0054B46D`),
    /// after which an unloading owner gives up its Unload
    /// ([`Simulation::jumpjet_move_to`]).
    pub lifted: bool,
}

/// What `Stop_Moving` did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StopOutcome {
    /// The moving byte was clear, so nothing ran (`0x0054B4EC`).
    Idle,
    /// The search answered a cell and `Move_To` ran on it (`0x0054B683`).
    Retargeted(MoveToOutcome),
    /// The search answered NullCell (`0x0054B68F`). A live owner then takes
    /// the C4 damage and loses its destination, which the caller runs.
    SearchFailed,
}

/// The owner and map seams `Move_To` and `Stop_Moving` reach. Coordinates are
/// world leptons.
pub(crate) trait JumpjetOrderHost {
    /// Owner virtual+F4 at the retained landing destination, before either
    /// order changes the destination or calls FNPC.
    fn withdraw_landing(&mut self, destination: DriveCoord);
    /// Owner RTTI (vtable `+0x2C`).
    fn owner_kind(&self) -> FlightOwnerKind;
    /// `TechnoTypeClass+0xD6A` `BalloonHover=`.
    fn balloon_hover(&self) -> bool;
    /// Owner `+0xB4` (the queued mission) or `GetCurrentMission` (vtable
    /// `+0x184`, `0x005B3040`) is Enter (7).
    fn mission_is_enter(&self) -> bool;
    /// Owner `+0x9C`.
    fn location(&self) -> DriveCoord;
    /// `MapClass::Find_Nearby_Passable_Cell @ 0x0056DC20` as `Stop_Moving`
    /// asks it (`0x0054B549..0x0054B5DF`): the owner type's `SpeedType=`
    /// (`+0x67C`) and `MovementZone=` (`+0x5B4`), zone -1, a 1x1 footprint,
    /// `alt` clear, bridges allowed and no target cell. `None` is NullCell.
    fn stop_search(&mut self, seed: (i16, i16)) -> Option<(i16, i16)>;
    /// The same search as `Move_To` asks it (`0x0054B275..0x0054B36F`):
    /// SpeedType Hover (3), MovementZone Fly (9), zone -1, 1x1 and no target
    /// cell, with the sixth (`alt`) and twelfth (bridges allowed) arguments
    /// given. `None` is NullCell.
    fn move_search(
        &mut self,
        seed: (i16, i16),
        alt: bool,
        allow_bridge: bool,
    ) -> Option<(i16, i16)>;
    /// XY of `CellClass::GetCoords` (vtable `+0x48`, `0x00486840`) for the
    /// cell `MapClass::Get_CellClass_At_Coord @ 0x00565730` finds at `xy`.
    fn cell_coords(&self, xy: [i32; 2]) -> [i32; 2];
    /// `0x00578080`: the floor height at a coordinate, deck excluded.
    fn floor_height(&self, xy: [i32; 2]) -> i32;
    /// `CellClass+0x140 & 0x100` of the cell holding `xy`.
    fn cell_high_bridge(&self, xy: [i32; 2]) -> bool;
    /// `0x0054D6D0`'s arm for an owner that is not a Unit: `0x004ACA10`
    /// picks the sub-cell, drawing on the Scenario RNG, and the floor is
    /// sampled again at its XY ([`infantry_destination_coordinate`]).
    fn infantry_destination(&mut self, centre: DriveCoord) -> Option<DriveCoord>;
    /// Owner `+0x5A4` NavCom: the cell holding `destination`
    /// (`0x0054B43A..0x0054B44B`).
    fn set_nav_com(&mut self, destination: DriveCoord);
}

/// Owner `+0xB4` (the queued mission) or `GetCurrentMission` (vtable
/// `+0x184`, `0x005B3040`) is Enter (7): the test both orders and State 4's
/// landing admission (`0x0054C6C5..0x0054C6D9`) make.
pub(crate) fn owner_mission_is_enter(owner: &crate::sim::game_entity::GameEntity) -> bool {
    owner.mission.queued().raw() == 7 || owner.mission.effective().raw() == 7
}

fn cell_centre(cell: (i16, i16)) -> [i32; 2] {
    [i32::from(cell.0) * 256 + 128, i32::from(cell.1) * 256 + 128]
}

/// `0x0054D6D0`'s Unit arm: the `GetCoords` centre of the cell the coordinate
/// falls in, at the floor height there (`0x0054D746`), on the deck over a high
/// bridge (`0x0054D759..0x0054D776`). `GetCoords` never answers NullCoord for
/// a cell, so the arm's NullCoord return (`0x0054D736`) is unreachable.
fn unit_destination(centre: DriveCoord, host: &impl JumpjetOrderHost) -> DriveCoord {
    let [x, y] = host.cell_coords([centre.x, centre.y]);
    let mut z = host.floor_height([x, y]);
    if host.cell_high_bridge([x, y]) {
        z = z.wrapping_add(crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS);
    }
    DriveCoord { x, y, z }
}

impl JumpjetRuntime {
    pub(crate) const NULL: DriveCoord = DriveCoord { x: 0, y: 0, z: 0 };

    pub(crate) fn destination(&self) -> DriveCoord {
        self.destination
    }

    pub(crate) fn moving(&self) -> bool {
        self.moving
    }

    pub(crate) fn phase(&self) -> i32 {
        self.phase
    }

    pub(crate) fn params(&self) -> jumpjet_flight::JumpjetFlightParams {
        self.params
    }

    pub(crate) fn flight(&self) -> jumpjet_flight::JumpjetFlight {
        self.flight
    }

    pub(crate) fn landing_latched(&self) -> bool {
        self.landing_latched
    }

    /// Publish the in-progress instance before an owner callback, then keep
    /// the instance that callback leaves installed. State1's class setter
    /// returns at54BB32 before Mark and the phase3 write; State3's tail at
    ///54C4FD reads the destination that setter changed. The working value is
    /// never an independent destination authority across either callback.
    pub(crate) fn with_owner_call<R>(
        &mut self,
        sim: &mut Simulation,
        id: u64,
        call: impl FnOnce(&mut Simulation) -> R,
    ) -> R {
        *sim.substrate
            .entities
            .get_mut(id)
            .and_then(|entity| entity.locomotor.as_mut())
            .and_then(|locomotor| locomotor.jumpjet_runtime_mut())
            .expect("Jumpjet callback retains its installed instance") = self.clone();
        let result = call(sim);
        *self = sim
            .substrate
            .entities
            .get(id)
            .and_then(|entity| entity.locomotor.as_ref())
            .and_then(|locomotor| locomotor.jumpjet_runtime())
            .expect("Jumpjet callback retains its installed instance")
            .clone();
        result
    }

    #[cfg(test)]
    pub(crate) fn with_destination_for_test(mut self, destination: DriveCoord) -> Self {
        self.destination = destination;
        self
    }

    #[cfg(test)]
    pub(crate) fn with_moving_for_test(mut self, moving: bool) -> Self {
        self.moving = moving;
        self
    }

    #[cfg(test)]
    pub(crate) fn with_phase_for_test(mut self, phase: i32) -> Self {
        self.phase = phase;
        self
    }

    #[cfg(test)]
    pub(crate) fn with_params_for_test(
        mut self,
        params: jumpjet_flight::JumpjetFlightParams,
    ) -> Self {
        self.params = params;
        self
    }

    #[cfg(test)]
    pub(crate) fn with_flight_for_test(mut self, flight: jumpjet_flight::JumpjetFlight) -> Self {
        self.flight = flight;
        self
    }

    #[cfg(test)]
    pub(crate) fn with_landing_latched_for_test(mut self, landing_latched: bool) -> Self {
        self.landing_latched = landing_latched;
        self
    }

    /// Jumpjet54D9B0, before Foot4DBDF0 applies its full NullCoord fallback.
    pub(crate) fn coordinate(&self, current: DriveCoord) -> DriveCoord {
        if self.phase == 0 {
            current
        } else {
            self.destination
        }
    }

    /// `Link_To_Object @ 0x0054AD30`: copy the type block and rebuild the
    /// locomotor facing at the type's turn rate, snapped to `0x4000`.
    pub(crate) fn link(&mut self, type_params: &crate::rules::jumpjet_params::JumpjetParams) {
        self.params = jumpjet_flight::JumpjetFlightParams::link(type_params);
        self.flight = jumpjet_flight::JumpjetFlight::linked(&self.params);
    }

    /// The prologue both orders share (`0x0054B1C3..0x0054B21A`,
    /// `0x0054B4F2..0x0054B537`): a landing State 4 admitted is withdrawn
    /// before the destination moves. Native withdraws it through the owner's
    /// vtable `+0xF4` (a Unit clears its cell occupation bit `0x20`,
    /// `0x00744210`). The callback precedes clearing the admission byte.
    fn withdraw_landing(&mut self, host: &mut impl JumpjetOrderHost) {
        if self.destination != Self::NULL && self.phase != 0 && self.landing_latched {
            host.withdraw_landing(self.destination);
            self.landing_latched = false;
        }
    }

    /// `Mark_All_Occupation_Bits @ 0x0054D930` with REMOVE, as Foot Limbo
    /// calls it (`0x004DB324`). Head_To_Coord (`0x0054D9B0`) is the owner's
    /// Location in State 0. The owner's vtable `+0xF4` runs there while
    /// grounded, or at Head_To_Coord for an admitted landing, and the landing
    /// byte clears (`0x0054D97B..0x0054D99C`). The null-coordinate skip
    /// (`0x0054D962..0x0054D979`) leaves the latch unchanged. The callback
    /// precedes its clear at54D99C. Nonzero/latch flow is instruction-established;
    /// the composed UnInit control executes the phase0 path.
    fn mark_all_occupation_bits_remove(
        &mut self,
        current: DriveCoord,
        remove: impl FnOnce(DriveCoord),
    ) -> bool {
        let head = self.coordinate(current);
        if head == Self::NULL || (self.phase != 0 && !self.landing_latched) {
            return false;
        }
        remove(head);
        self.landing_latched = false;
        true
    }

    /// `JumpjetLocomotionClass::Move_To @ 0x0054B1C0`.
    ///
    /// The request is stored first (`0x0054B22F`); NullCoord clears the moving
    /// byte (`0x0054B4C1`). Otherwise FNPC relocates the request's cell — a
    /// Unit that is not a balloon without bridges (`0x0054B275..0x0054B2D9`),
    /// everyone else with them and with the requested cell's high-bridge flag
    /// as `alt` (`0x0054B2DE..0x0054B369`) — and `0x0054D6D0` places the owner
    /// in the answer, NullCell included (`0x0054B374..0x0054B3B0`). A refused
    /// placement leaves the request and the moving byte as they were
    /// (`0x0054B3D4`). An accepted one replaces the request, except for a Unit
    /// that is a balloon or entering (`0x0054B3DA..0x0054B41E`), sets the
    /// NavCom unless entering (`0x0054B421..0x0054B44B`) and the moving byte
    /// (`0x0054B451`), and lifts a descent back into the climb at
    /// `JumpjetHeight=` (`0x0054B455..0x0054B467`).
    ///
    /// The host clears the same landing-for-deploy byte134 at54B46D
    /// before the lift ends Unload; the runtime owns only locomotion state.
    pub(crate) fn move_to(
        &mut self,
        request: DriveCoord,
        host: &mut impl JumpjetOrderHost,
    ) -> MoveToOutcome {
        self.withdraw_landing(host);
        self.destination = request;
        if request == Self::NULL {
            self.moving = false;
            return MoveToOutcome::default();
        }
        let unit = host.owner_kind() == FlightOwnerKind::Unit;
        let balloon = host.balloon_hover();
        let seed = coord_cell(request);
        let cell = if unit && !balloon {
            host.move_search(seed, false, false)
        } else {
            let alt = host.cell_high_bridge([request.x, request.y]);
            host.move_search(seed, alt, true)
        }
        .unwrap_or((0, 0));
        let [x, y] = cell_centre(cell);
        let centre = DriveCoord { x, y, z: 0 };
        let adjusted = if unit {
            Some(unit_destination(centre, host))
        } else {
            host.infantry_destination(centre)
        };
        let Some(adjusted) = adjusted.filter(|adjusted| *adjusted != Self::NULL) else {
            return MoveToOutcome::default();
        };
        let entering = host.mission_is_enter();
        if !unit || !(balloon || entering) {
            self.destination = adjusted;
        }
        if !entering {
            host.set_nav_com(self.destination);
        }
        self.moving = true;
        let lifted = self.phase == STATE_DESCEND;
        if lifted {
            self.phase = STATE_ASCEND;
            self.flight.target_height = self.params.height;
        }
        MoveToOutcome { lifted }
    }

    /// `JumpjetLocomotionClass::Stop_Moving @ 0x0054B4D0`: while moving, the
    /// nearest passable cell to the owner (`0x0054B5DF`) becomes a `Move_To`
    /// at its centre, at the floor height there and on the deck over a high
    /// bridge (`0x0054B605..0x0054B683`).
    ///
    /// Simulation's shared caller scope handles54B4D0's A8E7AC gate before
    /// this runtime body; factory/transport/destruction brackets also raise it.
    pub(crate) fn stop_moving(&mut self, host: &mut impl JumpjetOrderHost) -> StopOutcome {
        if !self.moving {
            return StopOutcome::Idle;
        }
        self.withdraw_landing(host);
        let here = host.location();
        let Some(cell) = host.stop_search(coord_cell(here)) else {
            return StopOutcome::SearchFailed;
        };
        let [x, y] = cell_centre(cell);
        let mut z = host.floor_height([x, y]);
        if host.cell_high_bridge([x, y]) {
            z = z.wrapping_add(crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS);
        }
        StopOutcome::Retargeted(self.move_to(DriveCoord { x, y, z }, host))
    }

    /// State0 handler54B980, after its owner callbacks. House53A130 is the
    /// original constantfalse leaf, not a house-policy input.
    ///
    /// Test-only since the locomotor took over the tick. Production reaches the
    /// same promotion through [`jumpjet_flight::state0_ground`], which
    /// `world::jumpjet_cruise` dispatches from `Process 0x0054AEC0`; that is the
    /// single authority. This mirror survives only because the
    /// `jumpjet_coordinates` corpus drives `activate` as one of its actions.
    #[cfg(test)]
    pub(crate) fn activate(&mut self) {
        if self.phase == 0 && self.moving {
            self.phase = 1;
        }
    }

    /// 54B6BC runs after the failed Stop search's damage callback.
    pub(crate) fn clear_failed_stop_destination(&mut self) {
        self.destination = Self::NULL;
    }
}

/// 54D6D0's Infantry (RTTI15) receiver: 4ACA10 selects the actual raw
/// subcell, then ground height is sampled AGAIN at the returned XY. The
/// second sample matters on slopes. No raw reservation is written here.
#[allow(clippy::too_many_arguments)]
pub(crate) fn infantry_destination_coordinate(
    input: DriveCoord,
    terrain: &crate::map::resolved_terrain::ResolvedTerrainGrid,
    raw: &crate::sim::occupancy::RawCellOccupationGrid,
    occupancy: &crate::sim::occupancy::OccupancyGrid,
    entities: &crate::sim::entity_store::EntityStore,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    interner: &crate::sim::intern::StringInterner,
    rng: &mut crate::sim::rng::SimRng,
) -> Option<DriveCoord> {
    use super::locomotor::MovementLayer;
    use crate::sim::{cell_kernel, occupancy::RawCellKey};
    let cell = terrain.native_cell_identity(((input.x / 256) as i16, (input.y / 256) as i16));
    let xy = terrain.native_cell_coord(cell);
    let level = match cell {
        crate::map::cell_index::NativeCellIdentity::Real(index) => terrain.cells()[index].level,
        crate::map::cell_index::NativeCellIdentity::Dummy => {
            terrain.shared_cell_dummy().snapshot().level as u8
        }
    };
    let bridge = cell_kernel::selects_infantry_bridge_layer(
        terrain.native_cell_flags(cell) & 0x100 != 0,
        level,
        input.z,
    );
    let key = RawCellKey::from_native(terrain, cell);
    let layer = if bridge {
        MovementLayer::Bridge
    } else {
        MovementLayer::Ground
    };
    let ground = raw.bits_at(key, MovementLayer::Ground);
    let selected = raw.bits_at(key, layer);
    let gate_open = selected & 0x20 == 0
        && ground & 0x40 != 0
        && super::bump_crush::ground_gate_is_open(
            occupancy,
            entities,
            rules,
            interner,
            (xy.0 as u16, xy.1 as u16),
        );
    let slot = super::bump_crush::place_infantry_in_native_cell(
        raw, key, layer, input, false, gate_open, rng,
    )?;
    let first_ground =
        super::ground_pose::ground_surface_z_at([input.x, input.y], false, Some(terrain), None)?;
    let mut adjusted = super::walk_head::selected_head(input, slot, first_ground, bridge);
    adjusted.z = super::ground_pose::ground_surface_z_at(
        [adjusted.x, adjusted.y],
        false,
        Some(terrain),
        None,
    )?;
    let selected_cell =
        terrain.native_cell_identity(((adjusted.x / 256) as i16, (adjusted.y / 256) as i16));
    if terrain.native_cell_flags(selected_cell) & 0x100 != 0 {
        adjusted.z = adjusted
            .z
            .wrapping_add(crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS);
    }
    Some(adjusted)
}

/// Mutable owner/map receiver for one class order. The raw landing callback
/// and NavCom publication execute before later reads in that same order.
struct WorldOrderHost<'a> {
    sim: &'a mut Simulation,
    id: u64,
    rules: Option<&'a crate::rules::ruleset::RuleSet>,
    path_grid: Option<std::sync::Arc<crate::sim::pathfinding::PathGrid>>,
    radius_cap: u16,
}

impl WorldOrderHost<'_> {
    fn owner(&self) -> &crate::sim::game_entity::GameEntity {
        self.sim
            .substrate
            .entities
            .get(self.id)
            .expect("Jumpjet order owner exists")
    }
    fn terrain(&self) -> &crate::map::resolved_terrain::ResolvedTerrainGrid {
        self.sim
            .resolved_terrain
            .as_ref()
            .expect("Jumpjet order map admitted")
    }
    fn search(
        &self,
        seed: (i16, i16),
        passability: crate::sim::find_nearby_cell::PassabilityArgs,
        allow_bridge_cells: bool,
    ) -> Option<(i16, i16)> {
        use crate::sim::find_nearby_cell::*;
        find_nearby_passable_cell(
            (i32::from(seed.0), i32::from(seed.1)),
            &NearbyQuery {
                native_cells: None,
                raw_occupation: Some(&self.sim.substrate.raw_cell_occupation),
                passability,
                footprint: NearbyFootprint::SINGLE,
                anchor_gate: NearbyAnchorGate::NativeHeightAware,
                allow_bridge_cells,
                check_height: false,
                check_occupancy: false,
                radius_cap: self.radius_cap,
                target_cell: None,
                path_grid: self.path_grid.as_deref(),
                resolved_terrain: Some(self.terrain()),
                overlay_grid: self.sim.overlay_grid.as_ref(),
                occupancy: Some(&self.sim.substrate.occupancy),
                entities: Some(&self.sim.substrate.entities),
                zone_grid: self.sim.zone_grid.as_ref(),
                playfield_bounds: self.sim.playfield_bounds,
            },
            self.sim.session.binary_frame,
        )
        .filter(|cell| *cell != (0, 0))
        .map(|(x, y)| (x as i16, y as i16))
    }
    fn cell_identity(&self, xy: [i32; 2]) -> crate::map::cell_index::NativeCellIdentity {
        self.terrain()
            .native_cell_identity((lepton_to_cell_packed(xy[0]), lepton_to_cell_packed(xy[1])))
    }
}

impl JumpjetOrderHost for WorldOrderHost<'_> {
    fn withdraw_landing(&mut self, destination: DriveCoord) {
        self.sim.object_raw_receiver_at(self.id, destination, false);
    }
    fn owner_kind(&self) -> FlightOwnerKind {
        FlightOwnerKind::of(self.owner().category)
    }
    fn balloon_hover(&self) -> bool {
        self.owner().locomotor.as_ref().unwrap().balloon_hover
    }
    fn mission_is_enter(&self) -> bool {
        owner_mission_is_enter(self.owner())
    }
    fn location(&self) -> DriveCoord {
        super::ground_pose::position_world_coord(&self.owner().position)
    }
    fn stop_search(&mut self, seed: (i16, i16)) -> Option<(i16, i16)> {
        let locomotor = self.owner().locomotor.as_ref().unwrap();
        let passability = crate::sim::find_nearby_cell::PassabilityArgs {
            speed_type: locomotor.speed_type,
            required_zone_id: None,
            movement_zone: locomotor.movement_zone,
            bridge_aware_zone: false,
        };
        self.search(seed, passability, true)
    }
    fn move_search(
        &mut self,
        seed: (i16, i16),
        alt: bool,
        allow_bridge: bool,
    ) -> Option<(i16, i16)> {
        self.search(
            seed,
            crate::sim::find_nearby_cell::PassabilityArgs {
                speed_type: crate::rules::locomotor_type::SpeedType::Hover,
                required_zone_id: None,
                movement_zone: crate::rules::locomotor_type::MovementZone::Fly,
                bridge_aware_zone: alt,
            },
            allow_bridge,
        )
    }
    fn cell_coords(&self, xy: [i32; 2]) -> [i32; 2] {
        cell_centre(self.terrain().native_cell_coord(self.cell_identity(xy)))
    }
    fn floor_height(&self, xy: [i32; 2]) -> i32 {
        super::ground_pose::ground_surface_z_at(xy, false, Some(self.terrain()), None).unwrap_or(0)
    }
    fn cell_high_bridge(&self, xy: [i32; 2]) -> bool {
        self.terrain().native_cell_flags(self.cell_identity(xy)) & 0x100 != 0
    }
    fn infantry_destination(&mut self, centre: DriveCoord) -> Option<DriveCoord> {
        infantry_destination_coordinate(
            centre,
            self.sim.resolved_terrain.as_ref().unwrap(),
            &self.sim.substrate.raw_cell_occupation,
            &self.sim.substrate.occupancy,
            &self.sim.substrate.entities,
            self.rules,
            &self.sim.interner,
            &mut self.sim.scenario_rng,
        )
    }
    fn set_nav_com(&mut self, destination: DriveCoord) {
        let (x, y) = self
            .terrain()
            .native_cell_coord(self.cell_identity([destination.x, destination.y]));
        self.sim
            .substrate
            .entities
            .get_mut(self.id)
            .unwrap()
            .navigation
            .nav_com = Some(crate::sim::components::NavTargetRef::cell(
            x as u16, y as u16,
        ));
    }
}

impl Simulation {
    /// Work on the installed Jumpjet instance through its owner. Reentrant
    /// class callbacks publish and reacquire this same instance through
    /// `with_owner_call`; the caller cannot commit separate flight fields.
    pub(crate) fn with_jumpjet_process<R>(
        &mut self,
        id: u64,
        process: impl FnOnce(&mut JumpjetRuntime, &mut Simulation) -> R,
    ) -> Option<R> {
        let mut runtime = self
            .substrate
            .entities
            .get(id)?
            .locomotor
            .as_ref()?
            .jumpjet_runtime()?
            .clone();
        let result = process(&mut runtime, self);
        *self
            .substrate
            .entities
            .get_mut(id)?
            .locomotor
            .as_mut()?
            .jumpjet_runtime_mut()? = runtime;
        Some(result)
    }

    /// Foot Limbo's first Limbo calls ILocomotion `+0x9C(0)` (`0x004DB324`):
    /// Jumpjet's [`JumpjetRuntime::mark_all_occupation_bits_remove`]. For an
    /// infantryman that is what clears his sub-cell: Infantry Mark clears none
    /// (`0x0047EAFE`).
    pub(crate) fn release_jumpjet_occupation_before_foot_limbo(&mut self, id: u64) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        if entity.lifecycle.in_limbo {
            return;
        }
        let eligible = entity.locomotor.as_ref().is_some_and(|locomotor| {
            locomotor.active_kind() == crate::rules::locomotor_type::LocomotorKind::Jumpjet
        });
        if !eligible {
            return;
        }
        let current = super::ground_pose::position_world_coord(&entity.position);
        self.with_jumpjet_process(id, |runtime, sim| {
            runtime.mark_all_occupation_bits_remove(current, |head| {
                sim.object_raw_receiver_at(id, head, false);
            });
        });
    }

    /// Run one Jumpjet order on a live owner over the world's grids. The
    /// private runtime retains reentrant writes; callbacks and NavCom writes
    /// execute over the live world in order. `None` for an owner
    /// without a Jumpjet runtime, or a world without a map to search.
    fn run_jumpjet_order<R>(
        &mut self,
        id: u64,
        rules: Option<&crate::rules::ruleset::RuleSet>,
        order: impl FnOnce(&mut JumpjetRuntime, &mut WorldOrderHost<'_>) -> R,
    ) -> Option<R> {
        let path_grid = self.path_grid_snapshot();
        let size = self
            .map_size_diamond()
            .or_else(|| self.bridge_state.as_ref()?.native_zone_source_size())?;
        self.resolved_terrain.as_ref()?;
        let radius_cap = crate::sim::find_nearby_cell::map_owned_radius_cap(size.0, size.1);
        self.with_jumpjet_process(id, |runtime, sim| {
            let mut host = WorldOrderHost {
                sim,
                id,
                rules,
                path_grid,
                radius_cap,
            };
            order(runtime, &mut host)
        })
    }

    /// [`JumpjetRuntime::move_to`] on a Jumpjet owner, then the lift's mission
    /// tail.
    pub(crate) fn jumpjet_move_to(
        &mut self,
        id: u64,
        request: DriveCoord,
        rules: Option<&crate::rules::ruleset::RuleSet>,
    ) -> Option<MoveToOutcome> {
        let outcome =
            self.run_jumpjet_order(id, rules, |runtime, host| runtime.move_to(request, host))?;
        if outcome.lifted {
            self.substrate
                .entities
                .get_mut(id)?
                .set_landing_for_deploy(false);
            self.jumpjet_lift_ends_unload(id, rules);
        }
        Some(outcome)
    }

    /// `Move_To`'s lift tail (`0x0054B479..0x0054B4B1`): an owner whose
    /// current mission (`GetCurrentMission`) is Unload queues Guard with
    /// `commence_now`, raises the queued-mission bypass `+0xB8` and commences
    /// it once ready. A readiness input VERA cannot evaluate leaves the mission
    /// as it was: the authority is atomic on a failed preflight.
    fn jumpjet_lift_ends_unload(
        &mut self,
        id: u64,
        rules: Option<&crate::rules::ruleset::RuleSet>,
    ) {
        use crate::sim::mission::authority::{EntityReadyInputProvider, LiveReadyInputProvider};
        let unloading = self.substrate.entities.get(id).is_some_and(|entity| {
            entity.mission.effective().known() == Some(crate::sim::mission::MissionType::Unload)
        });
        if !unloading {
            return;
        }
        let now = self.session.binary_frame;
        let _ = match rules {
            Some(rules) => self.mission_jumpjet_move_to_completion_exact(
                id,
                now,
                &LiveReadyInputProvider { rules },
            ),
            None => {
                self.mission_jumpjet_move_to_completion_exact(id, now, &EntityReadyInputProvider)
            }
        };
    }

    /// [`JumpjetRuntime::stop_moving`] on a Jumpjet owner, with what follows it
    /// on the owner: a lift ends an Unload, and a failed search kills a live
    /// owner with the C4 warhead before its destination is cleared. Answers
    /// false only when the world has no map to search, or a live owner's
    /// failed search has no rules for the damage; an owner without a Jumpjet
    /// locomotor answers true untouched.
    pub(crate) fn jumpjet_stop_moving(
        &mut self,
        id: u64,
        rules: Option<&crate::rules::ruleset::RuleSet>,
        registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
    ) -> bool {
        if self.object_placement_scope_active() {
            return true;
        }
        let jumpjet = self.substrate.entities.get(id).is_some_and(|entity| {
            entity
                .locomotor
                .as_ref()
                .and_then(|locomotor| locomotor.jumpjet_runtime())
                .is_some()
        });
        if !jumpjet {
            return true;
        }
        match self.run_jumpjet_order(id, rules, |runtime, host| runtime.stop_moving(host)) {
            None => false,
            Some(StopOutcome::Idle) => true,
            Some(StopOutcome::Retargeted(moved)) => {
                if moved.lifted {
                    if let Some(entity) = self.substrate.entities.get_mut(id) {
                        entity.set_landing_for_deploy(false);
                    }
                    self.jumpjet_lift_ends_unload(id, rules);
                }
                true
            }
            Some(StopOutcome::SearchFailed) => self.jumpjet_failed_stop(id, rules, registry),
        }
    }

    /// `Stop_Moving`'s failed search (`0x0054B68F..0x0054B6D3`): a dead owner
    /// is left as it is (`0x0054B698`); a live one takes `ReceiveDamage` of
    /// its own Health with `C4Warhead=`, ignoring defenses, and then loses its
    /// destination (`0x0054B6BC`).
    fn jumpjet_failed_stop(
        &mut self,
        id: u64,
        rules: Option<&crate::rules::ruleset::RuleSet>,
        registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
    ) -> bool {
        let Some(health) = self.substrate.entities.get(id).map(|e| e.health.current) else {
            return true;
        };
        if health <= 0 {
            return true;
        }
        let Some(rules) = rules else {
            return false;
        };
        use crate::sim::combat::{EntityDamageEvent, RAD_NO_ATTACKER, ReceiverCallFlags};
        let warhead = self.interner.intern(&rules.bridge_warheads.c4_name);
        // Native passes &Health. This uses the shared bridge_ground stock
        // C4Warhead=Super fatal-path quotient, not general aliased packet
        // support: override-only early damage-pointer writes remain outside it.
        self.commit_direct_damage_receiver(
            rules,
            registry,
            EntityDamageEvent::direct_receiver(
                id,
                i32::from(health),
                0,
                RAD_NO_ATTACKER,
                None,
                warhead,
                ReceiverCallFlags {
                    ignore_defenses: true,
                    arg6: true,
                },
            ),
        );
        // Read the retained live owner only AFTER synchronous damage/lifecycle.
        if let Some(state) = self
            .substrate
            .entities
            .get_mut(id)
            .and_then(|e| e.locomotor.as_mut())
            .and_then(|l| l.jumpjet_runtime_mut())
        {
            state.clear_failed_stop_destination();
        }
        true
    }

    /// The speed an order publishes with its destination: the running
    /// order's, else the owner's move speed.
    pub(crate) fn jumpjet_order_speed(
        &self,
        id: u64,
        rules: Option<&crate::rules::ruleset::RuleSet>,
    ) -> SimFixed {
        self.substrate
            .entities
            .get(id)
            .and_then(|entity| entity.movement_target.as_ref().map(|target| target.speed))
            .or_else(|| self.resolve_move_info(id, rules).map(|info| info.speed))
            .unwrap_or(SIM_ZERO)
    }

    /// The movement adapter's view of an order the locomotor accepted: the
    /// cell it now flies to, while its moving byte is set. The orders do not
    /// reconstruct Foot timers or retries; their accepted-destination caller
    /// publishes Foot's tail.
    pub(crate) fn publish_jumpjet_destination(&mut self, id: u64, speed: SimFixed) {
        use crate::sim::components::MovementTarget;
        let Some(entity) = self.substrate.entities.get_mut(id) else {
            return;
        };
        let Some(runtime) = entity
            .locomotor
            .as_ref()
            .and_then(|locomotor| locomotor.jumpjet_runtime())
        else {
            return;
        };
        if !runtime.moving {
            return;
        }
        let (x, y) = coord_cell(runtime.destination);
        let target = (x as u16, y as u16);
        entity.movement_target = Some(MovementTarget {
            speed,
            final_goal: Some(target),
        });
    }

    /// An air order to a cell. A Jumpjet Unit or Infantry takes Foot's setter
    /// ([`Self::jumpjet_cell_destination`]); other fliers continue through
    /// their established motion adapter.
    pub(crate) fn issue_air_cell_destination(
        &mut self,
        id: u64,
        target: (u16, u16),
        speed: SimFixed,
        rules: Option<&crate::rules::ruleset::RuleSet>,
    ) -> bool {
        if let Some(accepted) = self.jumpjet_cell_destination(id, target, speed, rules) {
            return accepted;
        }
        let Some(entity) = self.substrate.entities.get(id) else {
            return false;
        };
        let is_fly = entity
            .locomotor
            .as_ref()
            .and_then(|l| l.fly_runtime())
            .is_some();
        let coordinate = if is_fly {
            super::navcom::target_cell_coord(
                target.0,
                target.1,
                self.resolved_terrain
                    .as_ref()
                    .map(crate::map::resolved_terrain::NativeCellQuery::canonical)
                    .as_ref(),
            )
        } else {
            DriveCoord::cell(target.0, target.1, 0)
        };
        self.move_air_coordinate(
            id,
            coordinate,
            speed,
            Some(super::DestinationTiming::from_rules(
                self.session.binary_frame,
                rules,
            )),
            rules,
        )
    }

    /// A cell order to a Jumpjet Unit or Infantry, whichever route gave it:
    /// Foot's setter (`0x004D94B0`: the NavCom at `0x004D9510`, then the
    /// locomotor's `Move_To` with the cell's `GetCoords` at `0x004D965D`) and
    /// Foot's timer tail (`0x004D96C2`). Units enter their class destination
    /// owner, including its deployment refusal and existing power/warp gates.
    /// `None` when `id` is not a Jumpjet Unit or Infantry.
    ///
    /// RESIDUAL: before Foot's setter, Infantry's (`0x0051AA40`) swaps the
    /// Jumpjet of a `JumpJet=` infantry that is not moving for a Walk
    /// piggyback (`0x0051AE5A..0x0051B1CA`) unless `0x005221D0` says fly or
    /// the type has `HoverAttack=` (`+0x390`), and ends the piggyback when it
    /// says fly; a walking one queues the destination (`0x0051AD1F`).
    /// `0x005221D0` says fly when airborne; on the ground it says walk for a
    /// reachable hop of one cell, for two or three cells in the playfield
    /// whose zone cost is at most 7, and in two house and cell cases
    /// (`0x0053A130`, `0x00484AE0`). VERA flies every hop. Trigger: a grounded
    /// `JumpJet=` infantry without `HoverAttack=` given a short hop. Effect:
    /// it lifts off instead of walking. Frequency: dormant in retail, where
    /// each `JumpJet=` type also sets `HoverAttack=` (raw RULESMD read, not
    /// the production reader). Risk: a mod's walking jumpjet infantry fly.
    pub(crate) fn jumpjet_cell_destination(
        &mut self,
        id: u64,
        target: (u16, u16),
        speed: SimFixed,
        rules: Option<&crate::rules::ruleset::RuleSet>,
    ) -> Option<bool> {
        use crate::map::entities::EntityCategory;
        let entity = self.substrate.entities.get(id)?;
        let jumpjet = matches!(
            entity.category,
            EntityCategory::Infantry | EntityCategory::Unit
        ) && entity
            .locomotor
            .as_ref()
            .and_then(|l| l.jumpjet_runtime())
            .is_some();
        if !jumpjet {
            return None;
        }
        if entity.category == EntityCategory::Unit {
            // Every Unit caller uses the one class setter, including its
            // deployment refusal and Foot admission/timer tail. That owner
            // dispatches directly to Jumpjet Move_To, so this is not recursive.
            return Some(rules.is_some_and(|rules| {
                self.set_unit_destination(
                    id,
                    crate::sim::components::NavTargetRef::cell(target.0, target.1),
                    rules,
                    true,
                )
            }));
        }
        let Some(terrain) = self.resolved_terrain.as_ref() else {
            return Some(false);
        };
        let input = super::navcom::target_cell_coord(
            target.0,
            target.1,
            Some(&crate::map::resolved_terrain::NativeCellQuery::canonical(
                terrain,
            )),
        );
        let entity = self.substrate.entities.get_mut(id).expect("selected mover");
        super::navcom::publish_nav_com(
            entity,
            crate::sim::components::NavTargetRef::cell(target.0, target.1),
        );
        if self.jumpjet_move_to(id, input, rules).is_none() {
            return Some(false);
        }
        self.publish_jumpjet_destination(id, speed);
        if let Some(entity) = self.substrate.entities.get_mut(id) {
            // Infantry51B1D2 and Unit741970 return through Foot4D96C2 after
            // the locomotor's Move_To; the Move_To inside Stop cannot own this.
            super::DestinationTiming::from_rules(self.session.binary_frame, rules).accept(entity);
        }
        Some(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::resolved_terrain::ResolvedTerrainGrid;
    use crate::sim::{
        entity_store::EntityStore,
        intern::{InternedId, StringInterner},
        occupancy::{OccupancyGrid, RawCellOccupationGrid},
        rng::SimRng,
    };

    /// `0x0054D930` on REMOVE: a grounded owner's receiver runs; an airborne
    /// one's runs only for an admitted landing, whose byte clears either way.
    #[test]
    fn mark_all_occupation_remove_uses_the_retained_landing_head_and_null_gate() {
        let current = DriveCoord {
            x: 2688,
            y: 2688,
            z: 0,
        };
        let destination = DriveCoord {
            x: 3456,
            y: 2944,
            z: 0,
        };
        let mut calls = Vec::new();
        let mut runtime = JumpjetRuntime::default();
        assert!(runtime.mark_all_occupation_bits_remove(current, |head| calls.push(head)));
        runtime.phase = 4;
        runtime.destination = destination;
        assert!(!runtime.mark_all_occupation_bits_remove(current, |head| calls.push(head)));
        runtime.landing_latched = true;
        assert!(runtime.mark_all_occupation_bits_remove(current, |head| calls.push(head)));
        assert!(!runtime.landing_latched);
        runtime.destination = JumpjetRuntime::NULL;
        runtime.landing_latched = true;
        assert!(!runtime.mark_all_occupation_bits_remove(current, |head| calls.push(head)));
        assert!(runtime.landing_latched);
        assert_eq!(calls, [current, destination]);
    }

    /// The `jumpjet_coordinates` corpus owner: an Infantry at `(2496, 2624)`
    /// whose searches all answer cell (10,10), or NullCell when failing.
    struct CorpusHost<'a> {
        terrain: &'a ResolvedTerrainGrid,
        raw: &'a RawCellOccupationGrid,
        occupancy: &'a OccupancyGrid,
        entities: &'a EntityStore,
        interner: &'a StringInterner,
        rng: &'a mut SimRng,
        fail: bool,
    }

    impl JumpjetOrderHost for CorpusHost<'_> {
        fn withdraw_landing(&mut self, _destination: DriveCoord) {}
        fn owner_kind(&self) -> FlightOwnerKind {
            FlightOwnerKind::Infantry
        }
        fn balloon_hover(&self) -> bool {
            false
        }
        fn mission_is_enter(&self) -> bool {
            false
        }
        fn location(&self) -> DriveCoord {
            DriveCoord {
                x: 2496,
                y: 2624,
                z: 0,
            }
        }
        fn stop_search(&mut self, _seed: (i16, i16)) -> Option<(i16, i16)> {
            (!self.fail).then_some((10, 10))
        }
        fn move_search(
            &mut self,
            _seed: (i16, i16),
            _alt: bool,
            _allow_bridge: bool,
        ) -> Option<(i16, i16)> {
            (!self.fail).then_some((10, 10))
        }
        fn cell_coords(&self, xy: [i32; 2]) -> [i32; 2] {
            cell_centre((lepton_to_cell_packed(xy[0]), lepton_to_cell_packed(xy[1])))
        }
        fn floor_height(&self, xy: [i32; 2]) -> i32 {
            super::super::ground_pose::ground_surface_z_at(xy, false, Some(self.terrain), None)
                .unwrap_or(0)
        }
        fn cell_high_bridge(&self, xy: [i32; 2]) -> bool {
            let cell = self
                .terrain
                .native_cell_identity((lepton_to_cell_packed(xy[0]), lepton_to_cell_packed(xy[1])));
            self.terrain.native_cell_flags(cell) & 0x100 != 0
        }
        fn infantry_destination(&mut self, centre: DriveCoord) -> Option<DriveCoord> {
            infantry_destination_coordinate(
                centre,
                self.terrain,
                self.raw,
                self.occupancy,
                self.entities,
                None,
                self.interner,
                self.rng,
            )
        }
        fn set_nav_com(&mut self, _destination: DriveCoord) {}
    }

    /// Parity with `tools/spatial_oracle/jumpjet_coordinates.json`: the
    /// original `Move_To`, `Stop_Moving` (its search supplied, as the corpus
    /// supplies it) and the Infantry placement with its Scenario draws, step by
    /// step.
    #[test]
    fn retained_coordinates_and_placement_match_original_jumpjet_bodies() {
        use serde_json::json;
        let rows: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/jumpjet_coordinates.json",
        ))
        .unwrap();
        assert_eq!(rows.as_array().unwrap().len(), 12);
        for row in rows.as_array().unwrap() {
            let input = &row["input"];
            let mut terrain = ResolvedTerrainGrid::from_cells(
                11,
                11,
                (0..11)
                    .flat_map(|y| {
                        (0..11).map(move |x| {
                            crate::sim::world::common_raw_test_terrain_cell(x, y, 0, false)
                        })
                    })
                    .collect(),
            );
            let c = terrain.cell_mut(10, 10).unwrap();
            c.level = input["level"].as_u64().unwrap_or(2) as u8;
            c.slope_type = input["slope"].as_u64().unwrap_or(0) as u8;
            c.bridge_facts.raw_flags = if input["structural"].as_bool().unwrap_or(false) {
                0x100
            } else {
                0
            };
            let mut raw = RawCellOccupationGrid::default();
            raw.mark_ground_infantry(
                10,
                10,
                input["ground"].as_u64().unwrap_or(0) as u8,
                InternedId::from_index(0),
            );
            let occupancy = OccupancyGrid::new();
            let entities = EntityStore::new();
            let interner = StringInterner::new();
            let mut rng = SimRng::new(31);
            let current = DriveCoord {
                x: 2496,
                y: 2624,
                z: 0,
            };
            let mut state = JumpjetRuntime {
                phase: input["phase"].as_i64().unwrap_or(0) as i32,
                ..Default::default()
            };
            let snapshot = |state: &JumpjetRuntime| {
                let getter = state.coordinate(current);
                let foot = if getter == JumpjetRuntime::NULL {
                    current
                } else {
                    getter
                };
                json!({"destination":[state.destination.x,state.destination.y,state.destination.z],"moving":state.moving,"phase":state.phase,"getter":[getter.x,getter.y,getter.z],"foot":[foot.x,foot.y,foot.z]})
            };
            let mut trace = vec![snapshot(&state)];
            for action in input["actions"].as_array().unwrap() {
                let action = action.as_str().unwrap();
                let mut host = CorpusHost {
                    terrain: &terrain,
                    raw: &raw,
                    occupancy: &occupancy,
                    entities: &entities,
                    interner: &interner,
                    rng: &mut rng,
                    fail: action == "stop_fail",
                };
                match action {
                    "move" => {
                        let request = DriveCoord {
                            x: 2688,
                            y: 2688,
                            z: 900,
                        };
                        state.move_to(request, &mut host);
                    }
                    "stop" => {
                        state.stop_moving(&mut host);
                    }
                    "activate" => state.activate(),
                    "null" => {
                        state.move_to(JumpjetRuntime::NULL, &mut host);
                    }
                    // The corpus damage callback is supplied and Health is
                    // positive, so the clear follows the failed search.
                    "stop_fail" => {
                        if state.stop_moving(&mut host) == StopOutcome::SearchFailed {
                            state.clear_failed_stop_destination();
                        }
                    }
                    unknown => panic!("unknown action {unknown}"),
                }
                trace.push(snapshot(&state));
            }
            assert_eq!(json!(trace), row["output"]["trace"], "{input}");
            let logical = rng.logical_view();
            assert_eq!(
                json!([logical.index_a, logical.index_b]),
                row["output"]["random_indices"],
                "{input}"
            );
        }
    }
}
