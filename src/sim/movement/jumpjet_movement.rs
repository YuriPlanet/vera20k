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

use super::jumpjet_flight::{BRIDGE_DECK_LEPTONS, FlightOwnerKind, STATE_ASCEND, STATE_DESCEND};
use crate::sim::components::DriveCoord;
use crate::sim::world::Simulation;
use crate::util::fixed_math::{SIM_ZERO, SimFixed};

/// Persistent Jumpjet instance fields: cached XYZ at +40, moving at +4C,
/// phase at +50 (interface-relative offsets are four bytes smaller).
/// Constructor54AC40 and stream54B750/54B7E0 retain these independently.
/// AirMovePhase describes the older motion adapter and is not this authority.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct JumpjetRuntime {
    pub destination: DriveCoord,
    pub moving: bool,
    pub phase: i32,
    /// The type block `Link_To_Object @ 0x0054AD30` copies (`+0x1C..+0x3C`).
    pub params: super::jumpjet_flight::JumpjetFlightParams,
    /// Facing, speeds, target height and bob (`+0x54..+0x8C`).
    pub flight: super::jumpjet_flight::JumpjetFlight,
    /// Locomotor `+0x90`: State 4 sets this once it has admitted a landing, so
    /// later descent frames stop re-testing the cell. Touchdown clears it.
    #[serde(default)]
    pub landing_latched: bool,
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

/// The signed lepton-to-cell step (`(v + ((v >> 31) & 0xFF)) >> 8`) both
/// orders use.
fn native_cell(x: i32, y: i32) -> (i16, i16) {
    let step = |value: i32| (value.wrapping_add((value >> 31) & 0xFF) >> 8) as i16;
    (step(x), step(y))
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
        z = z.wrapping_add(BRIDGE_DECK_LEPTONS);
    }
    DriveCoord { x, y, z }
}

impl JumpjetRuntime {
    pub(crate) const NULL: DriveCoord = DriveCoord { x: 0, y: 0, z: 0 };

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
        self.params = super::jumpjet_flight::JumpjetFlightParams::link(type_params);
        self.flight = super::jumpjet_flight::JumpjetFlight::linked(&self.params);
    }

    /// The prologue both orders share (`0x0054B1C3..0x0054B21A`,
    /// `0x0054B4F2..0x0054B537`): a landing State 4 admitted is withdrawn
    /// before the destination moves. Native withdraws it through the owner's
    /// vtable `+0xF4` (a Unit clears its cell occupation bit `0x20`,
    /// `0x00744210`); VERA's admission raises only the latch
    /// (`world::jumpjet_cruise`), so only the latch drops here.
    fn withdraw_landing(&mut self) {
        if self.destination != Self::NULL && self.phase != 0 && self.landing_latched {
            self.landing_latched = false;
        }
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
    /// Not modelled: the lift's owner `+0x134` clear (`0x0054B46D`), which
    /// VERA has no field for (`world::jumpjet_cruise` reads it as clear).
    pub(crate) fn move_to(
        &mut self,
        request: DriveCoord,
        host: &mut impl JumpjetOrderHost,
    ) -> MoveToOutcome {
        self.withdraw_landing();
        self.destination = request;
        if request == Self::NULL {
            self.moving = false;
            return MoveToOutcome::default();
        }
        let unit = host.owner_kind() == FlightOwnerKind::Unit;
        let balloon = host.balloon_hover();
        let seed = native_cell(request.x, request.y);
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
    /// The `[0xA8E7AC]` gate at `0x0054B4D0` is raised only while a scenario
    /// is being set up, when no order or kill runs.
    pub(crate) fn stop_moving(&mut self, host: &mut impl JumpjetOrderHost) -> StopOutcome {
        if !self.moving {
            return StopOutcome::Idle;
        }
        self.withdraw_landing();
        let here = host.location();
        let Some(cell) = host.stop_search(native_cell(here.x, here.y)) else {
            return StopOutcome::SearchFailed;
        };
        let [x, y] = cell_centre(cell);
        let mut z = host.floor_height([x, y]);
        if host.cell_high_bridge([x, y]) {
            z = z.wrapping_add(BRIDGE_DECK_LEPTONS);
        }
        StopOutcome::Retargeted(self.move_to(DriveCoord { x, y, z }, host))
    }

    /// State0 handler54B980, after its owner callbacks. House53A130 is the
    /// original constantfalse leaf, not a house-policy input.
    ///
    /// Test-only since the locomotor took over the tick. Production reaches the
    /// same promotion through [`super::jumpjet_flight::state0_ground`], which
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
    let ground = raw.bits_at(key, MovementLayer::Ground);
    let selected = raw.bits_at(
        key,
        if bridge {
            MovementLayer::Bridge
        } else {
            MovementLayer::Ground
        },
    );
    let gate_open = selected & 0x20 == 0
        && ground & 0x40 != 0
        && occupancy
            .first_building_on_layer(xy.0 as u16, xy.1 as u16, MovementLayer::Ground)
            .and_then(|id| entities.get(id))
            .is_some_and(|building| {
                rules
                    .and_then(|rules| rules.object(interner.resolve(building.type_ref())))
                    .is_some_and(|object| object.gate)
                    && building
                        .building_gate
                        .is_some_and(|gate| gate.can_garrison_passable())
            });
    let slot = super::walk_head::select_slot(input, false, selected, ground, gate_open, rng)?;
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
        adjusted.z = adjusted.z.wrapping_add(416);
    }
    Some(adjusted)
}

/// The world's answers to [`JumpjetOrderHost`], borrowed for one order.
struct WorldOrderHost<'a> {
    terrain: &'a crate::map::resolved_terrain::ResolvedTerrainGrid,
    raw: &'a crate::sim::occupancy::RawCellOccupationGrid,
    occupancy: &'a crate::sim::occupancy::OccupancyGrid,
    entities: &'a crate::sim::entity_store::EntityStore,
    rules: Option<&'a crate::rules::ruleset::RuleSet>,
    interner: &'a crate::sim::intern::StringInterner,
    rng: &'a mut crate::sim::rng::SimRng,
    path_grid: Option<&'a crate::sim::pathfinding::PathGrid>,
    overlay_grid: Option<&'a crate::sim::overlay_grid::OverlayGrid>,
    zone_grid: Option<&'a crate::sim::pathfinding::zone_map::ZoneGrid>,
    playfield_bounds: Option<crate::sim::cell_rect::PlayfieldBounds>,
    radius_cap: u16,
    frame: u32,
    kind: FlightOwnerKind,
    balloon_hover: bool,
    entering: bool,
    location: DriveCoord,
    speed_type: crate::rules::locomotor_type::SpeedType,
    movement_zone: crate::rules::locomotor_type::MovementZone,
    /// The NavCom cell the order wrote, applied once the world is free again.
    nav_com: Option<(u16, u16)>,
}

impl WorldOrderHost<'_> {
    /// FNPC over the world's grids from `seed`, with the caller's passability
    /// and bridge admission.
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
                raw_occupation: Some(self.raw),
                passability,
                footprint: NearbyFootprint::SINGLE,
                anchor_gate: NearbyAnchorGate::NativeHeightAware,
                allow_bridge_cells,
                check_height: false,
                check_occupancy: false,
                radius_cap: self.radius_cap,
                target_cell: None,
                path_grid: self.path_grid,
                resolved_terrain: Some(self.terrain),
                overlay_grid: self.overlay_grid,
                occupancy: Some(self.occupancy),
                entities: Some(self.entities),
                zone_grid: self.zone_grid,
                playfield_bounds: self.playfield_bounds,
            },
            self.frame,
        )
        // Both orders compare the answer with NullCell, cell (0, 0).
        .filter(|cell| *cell != (0, 0))
        .map(|(x, y)| (x as i16, y as i16))
    }

    fn cell_identity(&self, xy: [i32; 2]) -> crate::map::cell_index::NativeCellIdentity {
        self.terrain.native_cell_identity(native_cell(xy[0], xy[1]))
    }
}

impl JumpjetOrderHost for WorldOrderHost<'_> {
    fn owner_kind(&self) -> FlightOwnerKind {
        self.kind
    }

    fn balloon_hover(&self) -> bool {
        self.balloon_hover
    }

    fn mission_is_enter(&self) -> bool {
        self.entering
    }

    fn location(&self) -> DriveCoord {
        self.location
    }

    fn stop_search(&mut self, seed: (i16, i16)) -> Option<(i16, i16)> {
        let passability = crate::sim::find_nearby_cell::PassabilityArgs {
            speed_type: self.speed_type,
            required_zone_id: None,
            movement_zone: self.movement_zone,
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
        let passability = crate::sim::find_nearby_cell::PassabilityArgs {
            speed_type: crate::rules::locomotor_type::SpeedType::Hover,
            required_zone_id: None,
            movement_zone: crate::rules::locomotor_type::MovementZone::Fly,
            bridge_aware_zone: alt,
        };
        self.search(seed, passability, allow_bridge)
    }

    fn cell_coords(&self, xy: [i32; 2]) -> [i32; 2] {
        cell_centre(self.terrain.native_cell_coord(self.cell_identity(xy)))
    }

    fn floor_height(&self, xy: [i32; 2]) -> i32 {
        super::ground_pose::ground_surface_z_at(xy, false, Some(self.terrain), None).unwrap_or(0)
    }

    fn cell_high_bridge(&self, xy: [i32; 2]) -> bool {
        self.terrain.native_cell_flags(self.cell_identity(xy)) & 0x100 != 0
    }

    fn infantry_destination(&mut self, centre: DriveCoord) -> Option<DriveCoord> {
        infantry_destination_coordinate(
            centre,
            self.terrain,
            self.raw,
            self.occupancy,
            self.entities,
            self.rules,
            self.interner,
            self.rng,
        )
    }

    fn set_nav_com(&mut self, destination: DriveCoord) {
        let (x, y) = self
            .terrain
            .native_cell_coord(self.cell_identity([destination.x, destination.y]));
        self.nav_com = Some((x as u16, y as u16));
    }
}

impl Simulation {
    /// Run one Jumpjet order on a live owner over the world's grids. The
    /// runtime is worked on a copy and written back with the NavCom the order
    /// set, so the host can borrow the rest of the world. `None` for an owner
    /// without a Jumpjet runtime, or a world without a map to search.
    fn run_jumpjet_order<R>(
        &mut self,
        id: u64,
        rules: Option<&crate::rules::ruleset::RuleSet>,
        order: impl FnOnce(&mut JumpjetRuntime, &mut WorldOrderHost<'_>) -> R,
    ) -> Option<R> {
        use crate::map::entities::EntityCategory;
        let path_grid = self.path_grid_snapshot();
        let size = self
            .map_size_diamond()
            .or_else(|| self.bridge_state.as_ref()?.native_zone_source_size())?;
        let terrain = self.resolved_terrain.as_ref()?;
        let entity = self.substrate.entities.get(id)?;
        let locomotor = entity.locomotor.as_ref()?;
        let mut runtime = locomotor.jumpjet_runtime()?.clone();
        let mut host = WorldOrderHost {
            terrain,
            raw: &self.substrate.raw_cell_occupation,
            occupancy: &self.substrate.occupancy,
            entities: &self.substrate.entities,
            rules,
            interner: &self.interner,
            rng: &mut self.scenario_rng,
            path_grid: path_grid.as_deref(),
            overlay_grid: self.overlay_grid.as_ref(),
            zone_grid: self.zone_grid.as_ref(),
            playfield_bounds: self.playfield_bounds,
            radius_cap: crate::sim::find_nearby_cell::map_owned_radius_cap(size.0, size.1),
            frame: self.session.binary_frame,
            kind: match entity.category {
                EntityCategory::Unit => FlightOwnerKind::Unit,
                EntityCategory::Infantry => FlightOwnerKind::Infantry,
                _ => FlightOwnerKind::Other,
            },
            balloon_hover: locomotor.balloon_hover,
            entering: entity.mission.queued().raw() == 7 || entity.mission.effective().raw() == 7,
            location: super::ground_pose::position_world_coord(&entity.position),
            speed_type: locomotor.speed_type,
            movement_zone: locomotor.movement_zone,
            nav_com: None,
        };
        let result = order(&mut runtime, &mut host);
        let nav_com = host.nav_com;
        let entity = self.substrate.entities.get_mut(id)?;
        if let Some((x, y)) = nav_com {
            entity.navigation.nav_com = Some(crate::sim::components::NavTargetRef::cell(x, y));
        }
        *entity.locomotor.as_mut()?.jumpjet_runtime_mut()? = runtime;
        Some(result)
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

    /// Foot's null `Set_Destination` (`0x004D94B0`) for a Jumpjet owner: the
    /// NavCom's auxiliary slot (`0x004D94C7`) and the NavCom (`0x004D9510`)
    /// clear, the locomotor's `Stop_Moving` runs (`0x004D96B9`), and the
    /// NavCom clears again (`0x004D96BC`), so the one its `Move_To` wrote does
    /// not survive. The Stop command, the Unit's null setter and an order VERA
    /// drops mid-flight reach it; the caller publishes Foot's timer tail.
    pub(crate) fn jumpjet_null_destination(
        &mut self,
        id: u64,
        rules: Option<&crate::rules::ruleset::RuleSet>,
        registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
    ) -> bool {
        let Some(entity) = self.substrate.entities.get_mut(id) else {
            return true;
        };
        if entity
            .locomotor
            .as_ref()
            .and_then(|locomotor| locomotor.jumpjet_runtime())
            .is_none()
        {
            return true;
        }
        entity.navigation.nav_com_aux = None;
        entity.navigation.nav_com = None;
        let stopped = self.jumpjet_stop_moving(id, rules, registry);
        if let Some(entity) = self.substrate.entities.get_mut(id) {
            entity.navigation.nav_com = None;
        }
        stopped
    }

    /// The Stun's `Stop_Driver` (`FootClass::Stun @ 0x004D5660` →
    /// `0x004D55C0`) on a dying Jumpjet Unit, run by the death arm and again
    /// by `FootClass::Crash`: `Stop_Moving` keeps a moving wreck flying to the
    /// cell under it and lifts a descent back into the climb, so `Process`'s
    /// crash latch engages wherever the kill found it. No Foot null setter
    /// follows, so the NavCom its `Move_To` wrote stays. The Stun's own null
    /// `Set_Destination` (`0x00741970`) reaches the same `Stop_Moving` while a
    /// NavCom is set, which is idempotent for a Unit: no Scenario draw, the
    /// same frame's search.
    ///
    /// A Jumpjet Infantry's Stun stops its locomotor through its own setter
    /// and Stop_Driver instead (`Simulation::techno_death_stun`).
    pub(crate) fn jumpjet_stun_stop(
        &mut self,
        id: u64,
        rules: Option<&crate::rules::ruleset::RuleSet>,
    ) {
        if self
            .substrate
            .entities
            .get(id)
            .is_some_and(|entity| entity.category == crate::map::entities::EntityCategory::Unit)
        {
            self.jumpjet_stop_moving(id, rules, None);
        }
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
        let (x, y) = native_cell(runtime.destination.x, runtime.destination.y);
        let target = (x as u16, y as u16);
        entity.movement_target = Some(MovementTarget {
            path: vec![target],
            path_layers: vec![super::locomotor::MovementLayer::Air],
            next_index: 0,
            speed,
            final_goal: Some(target),
            ..Default::default()
        });
    }

    /// An air order to a cell. A Jumpjet Unit or Infantry takes Foot's
    /// setter (`0x004D94B0`: the NavCom, then the locomotor's `Move_To` with
    /// the cell's `GetCoords`) and Foot's timer tail (`0x004D96C2`); a Unit
    /// keeps the Fly adapter's power and warp gates. Other fliers continue
    /// through their established motion adapter.
    pub(crate) fn issue_air_cell_destination(
        &mut self,
        id: u64,
        target: (u16, u16),
        speed: SimFixed,
        rules: Option<&crate::rules::ruleset::RuleSet>,
    ) -> bool {
        use crate::map::entities::EntityCategory;
        let Some(entity) = self.substrate.entities.get(id) else {
            return false;
        };
        let jumpjet = matches!(
            entity.category,
            EntityCategory::Infantry | EntityCategory::Unit
        ) && entity
            .locomotor
            .as_ref()
            .and_then(|l| l.jumpjet_runtime())
            .is_some();
        if !jumpjet {
            let is_fly = entity
                .locomotor
                .as_ref()
                .and_then(|l| l.fly_runtime())
                .is_some();
            let coordinate = if is_fly {
                super::navcom::target_cell_coord(target.0, target.1, self.resolved_terrain.as_ref())
            } else {
                DriveCoord::cell(target.0, target.1, 0)
            };
            return self.move_air_coordinate(
                id,
                coordinate,
                speed,
                Some(super::DestinationTiming::from_rules(
                    self.session.binary_frame,
                    rules,
                )),
                rules,
            );
        }
        if entity.category == EntityCategory::Unit
            && !super::air_movement::fly_coordinate_admitted(entity)
        {
            return false;
        }
        let Some(terrain) = self.resolved_terrain.as_ref() else {
            return false;
        };
        let input = super::navcom::target_cell_coord(target.0, target.1, Some(terrain));
        let entity = self.substrate.entities.get_mut(id).expect("selected mover");
        entity.navigation.nav_com = Some(crate::sim::components::NavTargetRef::cell(
            target.0, target.1,
        ));
        entity.navigation.nav_com_aux = None;
        if self.jumpjet_move_to(id, input, rules).is_none() {
            return false;
        }
        self.publish_jumpjet_destination(id, speed);
        if let Some(entity) = self.substrate.entities.get_mut(id) {
            // Infantry51B1D2 and Unit741970 return through Foot4D96C2 after
            // the locomotor's Move_To; the Move_To inside Stop cannot own this.
            super::DestinationTiming::from_rules(self.session.binary_frame, rules).accept(entity);
        }
        true
    }
}

/// Max cells for infantry walk fallback (TS-style jumpjet infantry).
const INFANTRY_WALK_THRESHOLD_CELLS: u32 = 3;

/// Whether a jumpjet infantry unit should use ground Walk for this move distance.
///
/// TS-style rule: infantry with Jumpjet + !HoverAttack walk for ≤3 cells.
pub fn should_use_walk_fallback(
    hover_attack: bool,
    is_infantry: bool,
    distance_cells: u32,
) -> bool {
    is_infantry && !hover_attack && distance_cells <= INFANTRY_WALK_THRESHOLD_CELLS
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
            cell_centre(native_cell(xy[0], xy[1]))
        }
        fn floor_height(&self, xy: [i32; 2]) -> i32 {
            super::super::ground_pose::ground_surface_z_at(xy, false, Some(self.terrain), None)
                .unwrap_or(0)
        }
        fn cell_high_bridge(&self, xy: [i32; 2]) -> bool {
            let cell = self.terrain.native_cell_identity(native_cell(xy[0], xy[1]));
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
        let rows: serde_json::Value = serde_json::from_str(include_str!(
            "../../../tools/spatial_oracle/jumpjet_coordinates.json"
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

    #[test]
    fn test_infantry_walk_fallback() {
        assert!(should_use_walk_fallback(false, true, 2));
        assert!(should_use_walk_fallback(false, true, 3));
        assert!(!should_use_walk_fallback(false, true, 4));
        assert!(!should_use_walk_fallback(true, true, 2)); // hover_attack blocks fallback
        assert!(!should_use_walk_fallback(false, false, 2)); // not infantry
    }
}
