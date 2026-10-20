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
//! - sim/ NEVER depends on render/, ui/, audio/, net/.

use super::foot_path::coord_cell;
use crate::sim::world::FrameEffects;
#[path = "jumpjet_flight.rs"]
pub mod jumpjet_flight;

use crate::sim::cell_kernel::{WorldCoordinate, cell_center};
use crate::sim::components::DriveCoord;
use crate::sim::world::Simulation;
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

    pub fn destination(&self) -> DriveCoord {
        self.destination
    }

    pub fn moving(&self) -> bool {
        self.moving
    }

    pub fn phase(&self) -> i32 {
        self.phase
    }

    pub fn params(&self) -> jumpjet_flight::JumpjetFlightParams {
        self.params
    }

    pub fn flight(&self) -> jumpjet_flight::JumpjetFlight {
        self.flight
    }

    pub fn landing_latched(&self) -> bool {
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

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn with_destination_for_test(mut self, destination: DriveCoord) -> Self {
        self.destination = destination;
        self
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn with_moving_for_test(mut self, moving: bool) -> Self {
        self.moving = moving;
        self
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn with_phase_for_test(mut self, phase: i32) -> Self {
        self.phase = phase;
        self
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn with_params_for_test(
        mut self,
        params: jumpjet_flight::JumpjetFlightParams,
    ) -> Self {
        self.params = params;
        self
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn with_flight_for_test(mut self, flight: jumpjet_flight::JumpjetFlight) -> Self {
        self.flight = flight;
        self
    }

    #[cfg(any(test, feature = "test-support"))]
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
        let centre = DriveCoord::from(cell_center(cell.into(), 0));
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
            self.flight.lift_to(self.params.height());
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
        let WorldCoordinate { x, y, .. } = cell_center(cell.into(), 0);
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
    #[cfg(any(test, feature = "test-support"))]
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

/// 54D6D0's arm for an owner that is null or not a Unit (`0x0054D6D9..E7`): 4ACA10
/// ([`super::bump_crush::pick_infantry_sublocation`]) selects the actual raw
/// subcell, then ground height is sampled AGAIN at the returned XY. The
/// second sample matters on slopes. No raw reservation is written here.
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
    let (slot, bridge) = super::bump_crush::pick_infantry_sublocation(
        Some(terrain),
        raw,
        occupancy,
        entities,
        rules,
        interner,
        rng,
        input,
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
        use crate::sim::find_nearby_cell::{NearbyArgs, NearbyFootprint};
        self.sim
            .nearby_passable_cell(
                (i32::from(seed.0), i32::from(seed.1)),
                NearbyArgs {
                    allow_bridge_cells,
                    ..NearbyArgs::plain(passability, NearbyFootprint::SINGLE)
                },
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
        Simulation::object_raw_receiver_at(
            &mut self.sim.substrate,
            &self.sim.path_grid,
            &self.sim.resolved_terrain,
            self.id,
            destination,
            false,
        );
    }
    fn owner_kind(&self) -> FlightOwnerKind {
        FlightOwnerKind::of(self.owner().category())
    }
    /// The owner type's `BalloonHover=` (`TechnoTypeClass+0xD6A`).
    fn balloon_hover(&self) -> bool {
        self.owner().locomotor.as_ref().unwrap().balloon_hover()
    }
    fn mission_is_enter(&self) -> bool {
        owner_mission_is_enter(self.owner())
    }
    /// The owner's Location (`+0x9C`).
    fn location(&self) -> DriveCoord {
        super::ground_pose::position_world_coord(&self.owner().position)
    }
    fn stop_search(&mut self, seed: (i16, i16)) -> Option<(i16, i16)> {
        let locomotor = self.owner().locomotor.as_ref().unwrap();
        let passability = crate::sim::find_nearby_cell::PassabilityArgs {
            speed_type: locomotor.speed_type(),
            required_zone_id: None,
            movement_zone: locomotor.movement_zone(),
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
        let cell = self.terrain().native_cell_coord(self.cell_identity(xy));
        let WorldCoordinate { x, y, .. } = cell_center(cell.into(), 0);
        [x, y]
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
    /// Move_To's store of the reached cell into NavCom (`0x0054B44B`).
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
            .store_nav_com(Some(crate::sim::components::NavTargetRef::cell(
                x as u16, y as u16,
            )));
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
        if entity.lifecycle.in_limbo() {
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
                Self::object_raw_receiver_at(
                    &mut sim.substrate,
                    &sim.path_grid,
                    &sim.resolved_terrain,
                    id,
                    head,
                    false,
                );
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
        self.map_size_diamond()?;
        self.resolved_terrain.as_ref()?;
        self.with_jumpjet_process(id, |runtime, sim| {
            let mut host = WorldOrderHost { sim, id, rules };
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
                .clear_landing_for_deploy();
            self.jumpjet_lift_ends_unload(id, rules);
        }
        Some(outcome)
    }

    /// `Move_To`'s lift tail (`0x0054B479..0x0054B4B1`): an owner whose
    /// current mission (`GetCurrentMission`) is Unload queues Guard with
    /// `commence_now`, raises the queued-mission bypass `+0xB8` and commences
    /// it once ready.
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
        let now = self.session.binary_frame();
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
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
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
                        entity.clear_landing_for_deploy();
                    }
                    self.jumpjet_lift_ends_unload(id, rules);
                }
                true
            }
            Some(StopOutcome::SearchFailed) => {
                self.jumpjet_failed_stop(id, rules, registry, frame_effects)
            }
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
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> bool {
        let Some(health) = self.substrate.entities.get(id).map(|e| e.health.current()) else {
            return true;
        };
        if health <= 0 {
            return true;
        }
        let Some(rules) = rules else {
            return false;
        };
        self.receive_own_health_c4(id, rules, registry, frame_effects);
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

    /// Foot's setter (`0x004D94B0`) after the Infantry setter's prelude, for a
    /// Jumpjet infantryman whose admission (`begin_foot_destination`) passed:
    /// the NavCom at `0x004D9510`, then the Jumpjet `Move_To` at `0x004D965D`
    /// with the coordinate the destination's `+0x4C` answers this mover at
    /// `0x004D9628`, then Foot's timer tail (`0x004D96C2`). A Cell answers its
    /// own coordinate, an object the one it gives this mover; no route is
    /// searched.
    pub(super) fn jumpjet_foot_destination(
        &mut self,
        id: u64,
        requested: crate::sim::components::NavTargetRef,
        rules: &crate::rules::ruleset::RuleSet,
    ) -> bool {
        let Some(terrain) = self.resolved_terrain.as_ref() else {
            return false;
        };
        let input = match super::navcom::nav_target_coordinate(
            requested,
            Some(id),
            self,
            Some(terrain),
            Some((rules, &self.interner)),
        ) {
            Ok(input) => input,
            Err(cause) => {
                log::debug!("Jumpjet {id} destination refused: {cause}");
                return false;
            }
        };
        let entity = self.substrate.entities.get_mut(id).expect("selected mover");
        super::navcom::publish_nav_com(entity, requested);
        if self.jumpjet_move_to(id, input, Some(rules)).is_none() {
            return false;
        }
        if let Some(entity) = self.substrate.entities.get_mut(id) {
            // Infantry51B1D2 returns through Foot4D96C2 after the locomotor's
            // Move_To; the Move_To inside Stop cannot own this.
            super::DestinationTiming::from_rules(self.session.binary_frame(), Some(rules))
                .accept(entity);
        }
        true
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
            let cell = (lepton_to_cell_packed(xy[0]), lepton_to_cell_packed(xy[1]));
            let WorldCoordinate { x, y, .. } = cell_center(cell.into(), 0);
            [x, y]
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
            let c = terrain.cell_mut_for_test(10, 10).unwrap();
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
