//! Transport unload handler tests: cadence, LIFO order, the pre-turn, the
//! IFV keep-one rule, the hover water→land pre-move, Move cancellation, the
//! Unload order's stop, the Is_Moving gate and the landed-only aircraft gate.

use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid};
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::rules::terrain_rules::SpeedCostProfile;
use crate::sim::command::Command;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::passenger::PassengerRole;
use crate::sim::pathfinding::PathGrid;
use crate::sim::pathfinding::passability::LandType;
use crate::sim::world::{ConcealOutcome, SimSoundEvent, Simulation};
use crate::util::fixed_math::{SIM_ZERO, SimFixed};

const OWNER: &str = "Americans";
const MAP: u16 = 40;

/// `[SHAD]` is an `[AircraftTypes]` entry here so tests reach the aircraft
/// Unload slot; retail lists it under `[VehicleTypes]`, as `[HIND]` is here.
fn rules() -> RuleSet {
    rules_with_patch("")
}

fn rules_with_patch(patch: &str) -> RuleSet {
    let mut ini = IniFile::from_str(
        "[InfantryTypes]\n0=E1\n[VehicleTypes]\n0=BFRT\n1=FV\n2=LCRF\n3=BGGY\n4=HIND\n\
         [AircraftTypes]\n0=SHAD\n\
         [BuildingTypes]\n[Countries]\n0=Americans\n\n\
         [General]\nFixtureOnly=1\n\n\
         [Unload]\nRate=.016\n[Move]\nRate=.016\n[Guard]\nRate=.03\n\n\
         [E1]\nStrength=125\nArmor=none\nSpeed=4\nSize=1\nIFVMode=1\n\
         Locomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\n\n\
         [BFRT]\nStrength=600\nArmor=heavy\nSpeed=4\nROT=5\nPassengers=5\nSizeLimit=2\n\
         OpenTopped=yes\nLeaveTransportSound=ExitTransport\n\
         Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\n\
         [FV]\nStrength=200\nArmor=light\nSpeed=4\nROT=5\nPassengers=3\nSizeLimit=1\nTurret=yes\n\
         TurretCount=4\nGunner=yes\nNormalTurretWeapon=0\nMachineGunTurretWeapon=1\nMachineGunTurretIndex=2\nLocomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\n\
         [BGGY]\nStrength=100\nArmor=light\nSpeed=6\nROT=5\nSize=1\n\
         Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\n\
         [LCRF]\nStrength=300\nArmor=light\nSpeed=6\nROT=5\nPassengers=12\nSizeLimit=12\nSpeedType=Hover\n\
         MovementZone=Amphibious\nLocomotor={4A582742-9839-11d1-B709-00A024DDAFD1}\n\n\
         [SHAD]\nStrength=200\nArmor=light\nSpeed=14\nROT=5\nPassengers=5\nLandable=yes\n\
         SizeLimit=2\nSpeedType=Hover\nMovementZone=Fly\n\
         Locomotor={92612C46-F71F-11d1-AC9F-006008055BB5}\n\n\
         [HIND]\nStrength=200\nArmor=light\nSpeed=14\nROT=5\nPassengers=5\nSizeLimit=2\n\
         SpeedType=Hover\nMovementZone=Fly\n\
         Locomotor={92612C46-F71F-11d1-AC9F-006008055BB5}\n",
    );
    ini.merge(&IniFile::from_str(patch));
    RuleSet::from_ini(&ini).expect("transport unload test rules parse")
}

/// Stock-shaped `[Clear]` speed row: passable for every land mover, not for
/// pure Float movers.
fn clear_costs() -> SpeedCostProfile {
    SpeedCostProfile {
        foot: Some(100),
        track: Some(100),
        wheel: Some(100),
        float: Some(0),
        amphibious: Some(100),
        float_beach: Some(100),
        hover: Some(100),
    }
}

fn clear_cell(rx: u16, ry: u16) -> ResolvedTerrainCell {
    ResolvedTerrainCell {
        speed_costs: clear_costs(),
        base_speed_costs: clear_costs(),
        allows_tiberium: true,
        ..crate::map::resolved_terrain::test_loader_clear_cell(rx, ry)
    }
}

fn water_cell(rx: u16, ry: u16) -> ResolvedTerrainCell {
    let mut cell = clear_cell(rx, ry);
    let water = LandType::Water.as_index();
    cell.land_type = water;
    cell.base_land_type = water;
    let costs = SpeedCostProfile {
        foot: Some(0),
        track: Some(0),
        wheel: Some(0),
        float: Some(100),
        amphibious: Some(100),
        float_beach: Some(100),
        hover: Some(100),
    };
    cell.speed_costs = costs;
    cell.base_speed_costs = costs;
    cell
}

fn flat_terrain(water: impl Fn(u16, u16) -> bool) -> ResolvedTerrainGrid {
    crate::map::resolved_terrain::test_grid(MAP, MAP, |rx, ry| {
        if water(rx, ry) {
            water_cell(rx, ry)
        } else {
            clear_cell(rx, ry)
        }
    })
}

struct Fixture {
    sim: Simulation,
    rules: RuleSet,
    grid: PathGrid,
}

impl Fixture {
    fn new(water: impl Fn(u16, u16) -> bool) -> Self {
        let rules = rules();
        let mut sim = Simulation::new();
        let span = i32::from(MAP);
        sim.set_playfield_bounds_for_test(Some(crate::map::playfield::PlayfieldBounds {
            base: 0,
            off_fc: -span,
            off_100: -span,
            off_104: span * 2,
            off_108: span * 2,
        }));
        sim.install_resolved_terrain_for_new_map(flat_terrain(water));
        let grid = PathGrid::test_all_passable(MAP, MAP);
        sim.install_fixture_path_grid(Some(&grid));
        // Hover's Find_Path and its zone precheck read the zone topology and
        // the Map Size.
        sim.set_playfield_size_height_for_test(Some(i32::from(MAP)));
        let terrain = sim.resolved_terrain.as_ref().expect("fixture terrain");
        sim.zone_grid = Some(
            crate::sim::pathfinding::zone_map::ZoneGrid::build_with_terrain(
                &grid,
                terrain,
                &[],
                MAP,
                MAP,
            ),
        );
        Self { sim, rules, grid }
    }

    fn spawn(&mut self, type_id: &str, rx: u16, ry: u16, facing: u8) -> u64 {
        self.sim
            .spawn_object(type_id, OWNER, rx, ry, facing, &self.rules)
            .unwrap_or_else(|| panic!("spawn {type_id}"))
    }

    /// Board `count` conscripts in spawn order (native `AddPassenger` prepends,
    /// so the last of these sits at the cargo head).
    fn board(&mut self, transport: u64, count: usize) -> Vec<u64> {
        let (trx, try_, open_topped) = {
            let t = self
                .sim
                .substrate
                .entities
                .get(transport)
                .expect("transport");
            let open_topped = self
                .sim
                .interner
                .object_type(t.type_ref(), &self.rules)
                .is_some_and(|object| object.open_topped);
            (t.position.rx(), t.position.ry(), open_topped)
        };
        let mut ids = Vec::new();
        for i in 0..count {
            let cell = (trx + 2 + i as u16, try_ + 2);
            let water = self
                .sim
                .resolved_terrain
                .as_ref()
                .unwrap()
                .cell(cell.0, cell.1)
                .unwrap()
                .land_type
                == LandType::Water.as_index();
            let pax = if water {
                // Supply already-limbo cargo for the landing-craft prestate.
                // Ordinary Foot Unlimbo correctly refuses this water row;
                // the measured unload starts with passengers already inside.
                self.sim
                    .spawn_object_limbo_at_height("E1", OWNER, cell.0, cell.1, 0, 0, &self.rules)
                    .unwrap()
            } else {
                let pax = self.spawn("E1", cell.0, cell.1, 0);
                assert_eq!(
                    self.sim.techno_limbo_with_rules(pax, &self.rules, None),
                    ConcealOutcome::Concealed
                );
                pax
            };
            let cargo = self
                .sim
                .substrate
                .entities
                .get_mut(transport)
                .and_then(|t| t.passenger_role.cargo_mut())
                .expect("transport cargo");
            assert!(cargo.can_accept(1.0), "cargo accepts passenger {i}");
            cargo.board_forced_chain(&[(pax, 1.0)]);
            self.sim
                .substrate
                .entities
                .get_mut(pax)
                .expect("passenger")
                .passenger_role = PassengerRole::inside_for_test(transport, open_topped);
            ids.push(pax);
        }
        ids
    }

    fn apply(&mut self, cmd: Command) -> bool {
        self.sim.apply_command(OWNER, &cmd, Some(&self.rules))
    }

    fn tick(&mut self) {
        let _ = self
            .sim
            .advance_tick(&[], Some(&self.rules), Some(&self.grid), None, 66);
    }

    fn frame(&self) -> u32 {
        self.sim.session.binary_frame()
    }

    fn revealed(&self, id: u64) -> bool {
        self.sim
            .substrate
            .entities
            .get(id)
            .is_some_and(|e| !e.lifecycle.in_limbo())
    }

    fn cargo_ids(&self, transport: u64) -> Vec<u64> {
        self.sim
            .substrate
            .entities
            .get(transport)
            .and_then(|t| t.passenger_role.cargo())
            .map(|c| c.passengers().to_vec())
            .unwrap_or_default()
    }

    fn mission(&self, id: u64) -> MissionId {
        self.sim
            .substrate
            .entities
            .get(id)
            .expect("entity")
            .mission
            .current()
    }

    /// The queued slot (`MissionClass +0xB4`): `Queue_Mission` writes here and
    /// `effective()` keeps reporting the current mission until `Commence`.
    fn mission_queued(&self, id: u64) -> MissionId {
        self.sim
            .substrate
            .entities
            .get(id)
            .expect("entity")
            .mission
            .queued()
    }

    fn facing(&self, id: u64) -> u8 {
        self.sim
            .substrate
            .entities
            .get(id)
            .expect("entity")
            .body_facing_byte(self.sim.session.binary_frame())
    }

    fn cell(&self, id: u64) -> (u16, u16) {
        let e = self.sim.substrate.entities.get(id).expect("entity");
        (e.position.rx(), e.position.ry())
    }

    /// Tick until every id is revealed, recording the frame each surfaced and
    /// the mission it held at that moment.
    fn run_until_revealed(&mut self, ids: &[u64], max_ticks: usize) -> Vec<(u64, u32)> {
        self.run_until_revealed_with_missions(ids, max_ticks)
            .into_iter()
            .map(|(id, frame, _)| (id, frame))
            .collect()
    }

    fn run_until_revealed_with_missions(
        &mut self,
        ids: &[u64],
        max_ticks: usize,
    ) -> Vec<(u64, u32, MissionId)> {
        let mut order = Vec::new();
        for _ in 0..max_ticks {
            self.tick();
            for &id in ids {
                if self.revealed(id) && !order.iter().any(|(seen, _, _)| *seen == id) {
                    let mission = self
                        .sim
                        .substrate
                        .entities
                        .get(id)
                        .expect("passenger")
                        .mission
                        .effective();
                    order.push((id, self.frame(), mission));
                }
            }
            if order.len() == ids.len() {
                break;
            }
        }
        order
    }
}

/// Drive4B20CD..4B2115 retires the arrival track before Unit PerCell739EC0
/// attaches a vehicle (73A6E8..73A793), so no track survives its Limbo and
/// Unlimbo.
#[test]
fn driven_vehicle_boards_at_arrival_and_unloads_at_the_moved_transport() {
    let mut fx = Fixture::new(|_, _| false);
    fx.rules = rules_with_patch("[LCRF]\nSizeLimit=6\n");
    check_vehicle_transport_trip(fx, "LCRF", "BGGY");
}

#[test]
fn retail_soviet_transport_unloads_driven_tank_at_the_new_shore() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let mut fx = Fixture::new(|_, _| false);
    fx.rules = retail.rules;
    check_vehicle_transport_trip(fx, "SAPC", "HTNK");
}

fn check_vehicle_transport_trip(mut fx: Fixture, carrier_type: &str, passenger_type: &str) {
    let transport = fx.spawn(carrier_type, 16, 16, 0);
    // Concurrent orders exercise the pending-entry wait while another
    // passenger owns the carrier's radio slot, not just an idle carrier.
    let passengers = [
        fx.spawn(passenger_type, 12, 16, 0),
        fx.spawn(passenger_type, 12, 18, 0),
    ];
    for passenger in passengers {
        assert!(fx.apply(Command::EnterTransport {
            passenger_id: passenger,
            transport_id: transport,
        }));
    }
    for _ in 0..600 {
        fx.tick();
        if passengers.iter().all(|&id| !fx.revealed(id)) {
            break;
        }
    }
    let cargo = fx.cargo_ids(transport);
    assert_eq!(
        cargo.len(),
        passengers.len(),
        "both queued vehicles boarded"
    );
    for passenger in passengers {
        assert!(cargo.contains(&passenger));
        let passenger_entity = fx.sim.substrate.entities.get(passenger).unwrap();
        assert_eq!(
            passenger_entity
                .locomotor
                .as_ref()
                .unwrap()
                .track_valid(crate::sim::movement::track_process::TrackFamily::Drive,),
            Some(false),
            "boarding must follow track completion, not physical cell crossing"
        );
    }

    assert!(fx.apply(Command::Move {
        entity_id: transport,
        target_rx: 26,
        target_ry: 26,
    }));
    for _ in 0..600 {
        fx.tick();
        if fx.cell(transport) == (26, 26)
            && !crate::sim::movement::motion_query::is_moving(
                fx.sim.substrate.entities.get(transport).unwrap(),
            )
            .expect("transport locomotor motion query")
        {
            break;
        }
    }
    assert_eq!(fx.cell(transport), (26, 26), "transport moved with cargo");
    assert!(fx.apply(Command::UnloadPassengers {
        transport_id: transport
    }));
    assert_eq!(
        fx.run_until_revealed(&passengers, 300).len(),
        passengers.len()
    );
    for _ in 0..120 {
        fx.tick();
    }
    for passenger in passengers {
        let unloaded = fx.cell(passenger);
        assert!(
            unloaded.0.abs_diff(26) <= 3 && unloaded.1.abs_diff(26) <= 3,
            "passenger must remain by the exit, not return to the boarding cell: {unloaded:?}"
        );
    }
    assert!(fx.cargo_ids(transport).is_empty());
}

/// Techno6F6ED2 and Foot4D72F4 are required inside the admitted Unlimbo,
/// before the ejected passenger receives its Move. Native reader712452
/// and nine-slot4FA2E0 arithmetic are saved in astar_threat_inputs.json;
/// astar_mtnk_inputs.json executes the original Foot coefficient-copy body.
/// This checks the real boarding/Limbo/departure consumers at one frame,
/// without claiming native equivalence of the whole transport mission.
#[test]
fn vehicle_unload_republishes_threat_and_foot_coefficient_before_movement() {
    use crate::sim::house_state::HouseState;
    use crate::util::native_x87::NativeF64Bits;

    let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/astar_threat_inputs.json",
    ))
    .unwrap();
    let reader = native["type_readers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["value"] == "0.5")
        .unwrap();
    let native_coefficient = u64::from_str_radix(reader["after"].as_str().unwrap(), 16)
        .unwrap()
        .swap_bytes();
    let adjustment = native["spatial"]["adjustments"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["amount"] == 7 && row["initial"] == 0)
        .unwrap();
    let native_slots: Vec<i32> = adjustment["after"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| i32::try_from(value.as_i64().unwrap()).unwrap())
        .collect();

    let mut fx = Fixture::new(|_, _| false);
    fx.rules = rules_with_patch("[E1]\nThreatPosed=7\nThreatAvoidanceCoefficient=0.5\n");
    assert_eq!(
        fx.rules
            .object("E1")
            .unwrap()
            .threat_avoidance_coefficient
            .bits(),
        native_coefficient,
        "production type reader agrees with original712452"
    );
    let owners: Vec<_> = [OWNER, "Russians"]
        .into_iter()
        .map(|name| {
            let owner = fx.sim.interner.intern(name);
            fx.sim
                .houses
                .register(HouseState::new(owner, 0, None, true, 0, 10));
            owner
        })
        .collect();
    let opposing_house = owners[1];
    fx.sim.houses.reorder_for_test(&owners);
    fx.sim
        .session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));

    // Already facing the exit scorer's selected hull direction: the actual
    // state0 -> state1 -> state3 producer can run without advancing any AI.
    let transport = fx.spawn("BFRT", 20, 20, 0x20);
    let passenger = fx.spawn("E1", 20, 19, 0);
    assert_eq!(
        fx.sim
            .substrate
            .entities
            .get(passenger)
            .unwrap()
            .cached_spatial_threat(),
        Some(7)
    );
    // This component comparison supplies physical arrival after the native
    // Enter order. Mobile infantry attaches at PerCellProcess519630.
    let arrival = crate::sim::movement::ground_pose::position_world_coord(
        &fx.sim.substrate.entities.get(transport).unwrap().position,
    );
    fx.sim.foot_set_location_marked(
        passenger,
        arrival,
        Some(&fx.rules),
        None,
        crate::sim::world::FrameEffects::default(),
    );
    let entity = fx.sim.substrate.entities.get_mut(passenger).unwrap();
    entity
        .navigation
        .set_nav_com_for_test(Some(crate::sim::components::NavTargetRef::object(
            transport,
        )));
    entity
        .mission
        .set_current_for_test(MissionId::from_known(MissionType::Enter), 0);
    fx.sim
        .per_cell_process(
            passenger,
            crate::sim::movement::PerCellReason::Arrival,
            Some(&fx.rules),
            None,
            crate::sim::world::FrameEffects::default(),
        )
        .unwrap();
    assert_eq!(fx.cargo_ids(transport), vec![passenger]);
    assert!(!fx.revealed(passenger));
    assert_eq!(
        fx.sim
            .substrate
            .entities
            .get(passenger)
            .unwrap()
            .cached_spatial_threat(),
        Some(0),
        "real transport entry removes the passenger's retained threat"
    );
    assert!(
        fx.sim
            .houses
            .get(&opposing_house)
            .unwrap()
            .spatial_threat_values()
            .iter()
            .all(|value| *value == 0)
    );
    // A retained Foot value may differ from its type; only admitted Unlimbo
    // owns refreshing it. No movement/coarse threat updater runs below.
    fx.sim
        .substrate
        .entities
        .get_mut(passenger)
        .unwrap()
        .navigation
        .retain_threat_avoidance_after_unlimbo(NativeF64Bits::ONE);
    assert!(fx.apply(Command::UnloadPassengers {
        transport_id: transport
    }));
    let frame = fx.frame();
    for _ in 0..3 {
        super::unit_mission_unload(
            &mut fx.sim,
            &fx.rules,
            transport,
            None,
            crate::sim::world::FrameEffects::default(),
        );
    }
    assert_eq!(fx.frame(), frame);
    assert!(
        fx.revealed(passenger),
        "real Unload admitted the cargo head"
    );
    assert!(fx.cargo_ids(transport).is_empty());

    let passenger = fx.sim.substrate.entities.get(passenger).unwrap();
    let cell = (
        passenger.position.rx() as i16,
        passenger.position.ry() as i16,
    );
    let center = crate::sim::house_threat::HouseSpatialThreat::index(cell);
    let actual_slots: Vec<_> = adjustment["offsets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|offset| {
            fx.sim
                .houses
                .get(&opposing_house)
                .unwrap()
                .spatial_threat_at_padded_index(
                    center + i32::try_from(offset.as_i64().unwrap()).unwrap(),
                )
                .unwrap()
        })
        .collect();
    assert_eq!(
        (
            passenger.navigation.path_threat_coefficient().bits(),
            passenger.cached_spatial_threat(),
            actual_slots
        ),
        (native_coefficient, Some(7), native_slots),
        "admitted departure must publish Foot+530, cached threat and all nine House slots before movement"
    );
}

/// Five conscripts in a BFRT: the hull starts turning, then one passenger leaves
/// per `[Unload] Rate` dispatch (14..16 frames apart), last boarded first.
#[test]
fn bfrt_unloads_one_per_dispatch_in_reverse_boarding_order_while_turning() {
    let mut fx = Fixture::new(|_, _| false);
    let bfrt = fx.spawn("BFRT", 20, 20, 0);
    let pax = fx.board(bfrt, 5);
    assert_eq!(
        fx.cargo_ids(bfrt),
        pax.iter().rev().copied().collect::<Vec<_>>()
    );

    assert!(fx.apply(Command::UnloadPassengers { transport_id: bfrt }));

    let order = fx.run_until_revealed_with_missions(&pax, 200);
    let ids: Vec<u64> = order.iter().map(|(id, _, _)| *id).collect();
    assert_eq!(
        ids,
        pax.iter().rev().copied().collect::<Vec<_>>(),
        "cargo head (last boarded) leaves first"
    );
    // Facing 0 (N): the scorer picks octant 5 (SW), so the transport faces
    // octant 1 (NE, 0x20). By the last passenger it has completed the arc.
    assert_eq!(
        fx.facing(bfrt),
        0x20,
        "hull turned to put its rear to the exit"
    );
    for pair in order.windows(2) {
        let gap = pair[1].1 - pair[0].1;
        assert!(
            (14..=16).contains(&gap),
            "one passenger per [Unload] Rate dispatch: gap {gap} frames"
        );
    }
    // Unit73D892 advances without waiting for the turretless hull's six-frame
    // arc. Ejection must precede its completion, without a wait epilogue.
    assert!(order[0].1 < 8, "first ejection at frame {}", order[0].1);
    // Each ejected passenger surfaced holding `Queue_Mission(Move)`.
    for (id, _, mission) in &order {
        assert_eq!(
            *mission,
            MissionId::from_known(MissionType::Move),
            "passenger {id} left on Move"
        );
    }
    let leave_sounds = fx
        .sim
        .sound_events
        .iter()
        .filter(|e| matches!(e, SimSoundEvent::LeaveTransport { rx: 20, ry: 20, .. }))
        .count();
    assert!(leave_sounds >= 1, "LeaveTransportSound at the transport");

    for _ in 0..40 {
        fx.tick();
    }
    assert_eq!(fx.mission(bfrt), MissionId::from_known(MissionType::Guard));
    assert!(fx.cargo_ids(bfrt).is_empty());
}

/// Unit73DBDB and Aircraft415C05 queue a passenger Move directly. A retained
/// Archive on an already-boarded passenger is a component prior, not a claim
/// that normal boarding creates it; these internal queues must preserve it.
#[test]
fn internal_ejection_preserves_passenger_archive() {
    use crate::sim::combat::TargetKind;

    for transport_type in ["BFRT", "SHAD"] {
        let mut fx = Fixture::new(|_, _| false);
        let transport = fx.spawn(transport_type, 20, 20, 0);
        let passenger = fx.board(transport, 1)[0];
        let retained = Some(TargetKind::Cell(9, 9));
        fx.sim
            .substrate
            .entities
            .get_mut(passenger)
            .unwrap()
            .set_archive_target(retained);
        assert!(fx.apply(Command::UnloadPassengers {
            transport_id: transport,
        }));
        for _ in 0..120 {
            fx.tick();
            if fx.revealed(passenger) {
                break;
            }
        }
        assert!(fx.revealed(passenger), "{transport_type}: passenger leaves");
        assert_eq!(
            fx.sim
                .substrate
                .entities
                .get(passenger)
                .unwrap()
                .archive_target(),
            retained,
            "{transport_type}: internal queue preserves passenger Archive"
        );
    }
}

/// The ejectors keep the passenger's target: Unit Mission_Unload clears it
/// only when the passenger leaves another house's OpenTopped transport,
/// through the class NULL setter (`0x0073DB9D..0x0073DBC3`), and KickOutUnit
/// has no target write (`0x00415BF9..0x00415C21`). The passenger's own AI
/// may drop the target later; this checks what the ejector leaves.
#[test]
fn ejection_keeps_the_passengers_target_unless_it_leaves_another_houses_open_transport() {
    use crate::sim::combat::TargetKind;

    for (transport_type, other_house, keeps) in [
        ("BFRT", false, true),
        ("BFRT", true, false),
        ("LCRF", true, true),
        ("SHAD", false, true),
    ] {
        let mut fx = Fixture::new(|_, _| false);
        let transport = fx.spawn(transport_type, 20, 20, 0);
        let passenger = fx.board(transport, 1)[0];
        let target = Some(TargetKind::Cell(9, 9));
        let other = fx.sim.intern("Russians");
        let pax = fx.sim.substrate.entities.get_mut(passenger).unwrap();
        pax.attack_target.set_for_test(target);
        pax.passive_scan
            .set_passively_acquired_target_for_test(true);
        pax.weapon_burst.complete_shot(3);
        if other_house {
            fx.sim.substrate.entities.change_owner(passenger, other);
        }
        let effects = crate::sim::world::FrameEffects::default();
        if transport_type == "SHAD" {
            assert!(super::eject_from_aircraft(
                &mut fx.sim,
                &fx.rules,
                None,
                transport,
                effects
            ));
        } else {
            assert!(matches!(
                super::eject_head_passenger(&mut fx.sim, &fx.rules, transport, None, effects),
                super::EjectOutcome::Placed
            ));
        }
        assert!(fx.revealed(passenger), "{transport_type}: passenger leaves");
        let pax = fx.sim.substrate.entities.get(passenger).unwrap();
        let case = format!("{transport_type}, other house {other_house}");
        if keeps {
            assert_eq!(pax.attack_target.target(), target, "{case}");
            assert!(pax.passive_scan.acquired(), "{case}");
            assert_eq!(pax.weapon_burst.index(), 1, "{case}");
        } else {
            // Through the class setter: its NULL arm resets the burst
            // (`0x006FCF5B`), which a raw store would not.
            assert_eq!(pax.attack_target.target(), None, "{case}");
            assert!(!pax.passive_scan.acquired(), "{case}");
            assert_eq!(pax.weapon_burst.index(), 0, "{case}");
        }
    }
}

/// `TurretCount > 0`: `+0x6E4 = (cargo == 1 ? 0 : 1)` — the IFV keeps its
/// last passenger (the first boarded, at the cargo tail).
#[test]
fn turreted_transport_keeps_its_last_passenger() {
    let mut fx = Fixture::new(|_, _| false);
    let fv = fx.spawn("FV", 20, 20, 0);
    let pax = fx.board(fv, 3);
    assert!(fx.apply(Command::UnloadPassengers { transport_id: fv }));
    for _ in 0..120 {
        fx.tick();
    }
    assert_eq!(fx.cargo_ids(fv), vec![pax[0]], "first boarded stays aboard");
    assert!(fx.revealed(pax[1]) && fx.revealed(pax[2]));
    assert_eq!(fx.mission(fv), MissionId::from_known(MissionType::Guard));

    // Exactly one aboard: keep-count 0, it unloads.
    let mut fx = Fixture::new(|_, _| false);
    let fv = fx.spawn("FV", 20, 20, 0);
    let pax = fx.board(fv, 1);
    assert!(fx.apply(Command::UnloadPassengers { transport_id: fv }));
    for _ in 0..80 {
        fx.tick();
    }
    assert!(fx.revealed(pax[0]));
    assert!(fx.cargo_ids(fv).is_empty());
}

/// A hover landing craft on water drives to land first (`LandType == 2` and
/// no NavCom → `Find_Nearby_Passable_Cell` + `Set_Destination`, `return 10`),
/// then unloads onto land.
#[test]
fn hover_transport_on_water_moves_to_shore_before_unloading() {
    let mut fx = Fixture::new(|rx, _| rx < 25);
    let lcrf = fx.spawn("LCRF", 20, 20, 0x40);
    let pax = fx.board(lcrf, 2);
    assert!(fx.apply(Command::UnloadPassengers { transport_id: lcrf }));

    for _ in 0..3 {
        fx.tick();
    }
    let moving = fx.sim.substrate.entities.get(lcrf).is_some_and(|e| {
        crate::sim::movement::motion_query::is_moving(&e) == Some(true)
            || e.navigation.nav_com().is_some()
    });
    assert!(moving, "the landing craft is driven toward land first");
    assert_eq!(
        fx.cargo_ids(lcrf).len(),
        2,
        "nobody is dropped into the water"
    );

    let order = fx.run_until_revealed(&pax, 600);
    assert_eq!(order.len(), 2, "both passengers landed: {order:?}");
    let (trx, _) = fx.cell(lcrf);
    assert!(
        trx >= 25,
        "transport reached land before unloading (x={trx})"
    );
    for &id in &pax {
        let (rx, _) = fx.cell(id);
        assert!(rx >= 25, "passenger {id} placed on land (x={rx})");
    }
}

/// The water-to-land pre-move pushes no height gate (`0x0073D782`): a bank
/// two levels above the water still answers Find_Nearby_Passable_Cell, so the
/// landing craft is sent to it instead of waiting on the water.
#[test]
fn hover_transport_on_water_is_sent_to_a_raised_bank() {
    let mut fx = Fixture::new(|rx, _| rx < 25);
    let terrain = fx.sim.resolved_terrain.as_mut().unwrap();
    for y in 0..MAP {
        for x in 25..MAP {
            terrain.cell_mut_for_test(x, y).unwrap().level = 2;
        }
    }
    let lcrf = fx.spawn("LCRF", 20, 20, 0x40);
    fx.board(lcrf, 2);
    assert!(fx.apply(Command::UnloadPassengers { transport_id: lcrf }));

    let mut sent = None;
    for _ in 0..10 {
        fx.tick();
        sent = fx
            .sim
            .substrate
            .entities
            .get(lcrf)
            .unwrap()
            .navigation
            .nav_com();
        if sent.is_some() {
            break;
        }
    }
    assert!(
        matches!(sent, Some(crate::sim::components::NavTargetRef::Cell { rx, .. }) if rx >= 25),
        "sent to the raised bank: {sent:?}"
    );
}

/// A Move order re-assigns the mission and interrupts the unload.
#[test]
fn move_order_cancels_a_running_unload() {
    let mut fx = Fixture::new(|_, _| false);
    let bfrt = fx.spawn("BFRT", 20, 20, 0);
    let pax = fx.board(bfrt, 5);
    assert!(fx.apply(Command::UnloadPassengers { transport_id: bfrt }));
    let mut out = 0usize;
    for _ in 0..60 {
        fx.tick();
        out = pax.iter().filter(|&&id| fx.revealed(id)).count();
        if out >= 2 {
            break;
        }
    }
    assert!((2..5).contains(&out), "unload in progress ({out} out)");

    assert!(fx.apply(Command::Move {
        entity_id: bfrt,
        target_rx: 30,
        target_ry: 30,
    }));
    for _ in 0..80 {
        fx.tick();
    }
    let now_out = pax.iter().filter(|&&id| fx.revealed(id)).count();
    assert_eq!(
        now_out, out,
        "no further passenger leaves after the Move order"
    );
    assert_eq!(fx.cargo_ids(bfrt).len(), 5 - out);
    assert_ne!(fx.mission(bfrt), MissionId::from_known(MissionType::Unload));
}

/// The Aircraft Unload slot ejects only once `GetHeight() == 0` and the
/// approach pitch `+0x2E8 == 0.0` (`0x004151FF`, `0x0041520D`): an airborne
/// Nighthawk holds its cargo, and so does a grounded one still pitched.
#[test]
fn nighthawk_unloads_only_when_landed() {
    let mut fx = Fixture::new(|_, _| false);
    let shad = fx.spawn("SHAD", 20, 20, 0);
    let pax = fx.board(shad, 2);
    fx.sim.set_object_height_unmarked(shad, 600);
    assert!(fx.apply(Command::UnloadPassengers { transport_id: shad }));
    assert_eq!(fx.mission(shad), MissionId::from_known(MissionType::Unload));
    for _ in 0..30 {
        fx.sim.set_object_height_unmarked(shad, 600);
        crate::sim::aircraft::dispatch_mission(&mut fx.sim, shad, &fx.rules, Default::default());
        fx.sim
            .session
            .set_binary_frame_for_test(fx.sim.session.binary_frame() + 1);
    }
    assert_eq!(fx.cargo_ids(shad).len(), 2, "airborne: nothing leaves");

    fx.sim.set_object_height_unmarked(shad, 0);
    let pitch = |fx: &mut Fixture, angle: SimFixed| {
        fx.sim
            .substrate
            .entities
            .get_mut(shad)
            .expect("aircraft")
            .flight_attitude
            .approach(0, 1, angle);
    };
    pitch(&mut fx, SimFixed::from_num(1));
    for _ in 0..30 {
        crate::sim::aircraft::dispatch_mission(&mut fx.sim, shad, &fx.rules, Default::default());
        fx.sim
            .session
            .set_binary_frame_for_test(fx.sim.session.binary_frame() + 1);
    }
    assert_eq!(fx.cargo_ids(shad).len(), 2, "pitched: nothing leaves");

    pitch(&mut fx, SIM_ZERO);
    let mut frames = Vec::new();
    for _ in 0..80 {
        crate::sim::aircraft::dispatch_mission(&mut fx.sim, shad, &fx.rules, Default::default());
        fx.sim
            .session
            .set_binary_frame_for_test(fx.sim.session.binary_frame() + 1);
        for &id in &pax {
            if fx.revealed(id) && !frames.iter().any(|(seen, _)| *seen == id) {
                frames.push((id, fx.frame()));
            }
        }
    }
    assert_eq!(
        frames.len(),
        2,
        "landed: both passengers leave ({frames:?})"
    );
    assert_eq!(frames[0].0, pax[1], "cargo head first");
    let gap = frames[1].1 - frames[0].1;
    assert!((14..=16).contains(&gap), "one per [Unload] dispatch: {gap}");
    for &id in &pax {
        assert_eq!(fx.cell(id), (20, 20), "placed at the aircraft's cell");
    }
    assert!(fx.cargo_ids(shad).is_empty());
}

/// `0x00415B10`: five conscripts leave a landed Nighthawk one per `[Unload]`
/// dispatch, each `Unlimbo`ed at the aircraft's own cell and sent on a Move
/// to the first table-order neighbour (`DAT_00817A58`: S, SW, SE, NW, NE, N,
/// W, E) its `Can_Enter_Cell` accepted; the aircraft ends up in Guard.
/// Native gives no distinctness guarantee across passengers — the scan
/// restarts at S every time — so the assertion is membership in the eight
/// neighbours plus the table's first entry for the first passenger.
#[test]
fn nighthawk_ejects_five_passengers_to_scanned_neighbours() {
    let mut fx = Fixture::new(|_, _| false);
    let shad = fx.spawn("SHAD", 20, 20, 0);
    let pax = fx.board(shad, 5);
    fx.sim.set_object_height_unmarked(shad, 0);
    assert!(fx.apply(Command::UnloadPassengers { transport_id: shad }));

    // (id, reveal frame, mission, destination) captured the tick each surfaces.
    type Surfaced = (u64, u32, MissionId, Option<(u16, u16)>);
    let mut out: Vec<Surfaced> = Vec::new();
    for _ in 0..200 {
        fx.tick();
        for &id in &pax {
            if fx.revealed(id) && !out.iter().any(|(seen, ..)| *seen == id) {
                let e = fx.sim.substrate.entities.get(id).expect("passenger");
                let dest =
                    crate::sim::movement::movement_goal_cell(e).or(match e.navigation.nav_com() {
                        Some(crate::sim::components::NavTargetRef::Cell { rx, ry }) => {
                            Some((rx, ry))
                        }
                        _ => None,
                    });
                out.push((id, fx.frame(), e.mission.effective(), dest));
            }
        }
        if out.len() == pax.len() {
            break;
        }
    }
    assert_eq!(out.len(), 5, "all five passengers ejected: {out:?}");
    assert_eq!(
        out.iter().map(|(id, ..)| *id).collect::<Vec<_>>(),
        pax.iter().rev().copied().collect::<Vec<_>>(),
        "cargo head first"
    );
    for pair in out.windows(2) {
        let gap = pair[1].1 - pair[0].1;
        assert!(
            (14..=16).contains(&gap),
            "one per [Unload] dispatch: gap {gap} ({out:?})"
        );
    }
    let neighbours: Vec<(u16, u16)> = super::DIRECTION_DELTAS
        .iter()
        .map(|(dx, dy)| ((20 + dx) as u16, (20 + dy) as u16))
        .collect();
    for (id, _, mission, dest) in &out {
        assert_eq!(
            *mission,
            MissionId::from_known(MissionType::Move),
            "passenger {id} left on Queue_Mission(Move)"
        );
        let dest = dest.unwrap_or_else(|| panic!("passenger {id} has a Move destination"));
        assert!(
            neighbours.contains(&dest),
            "passenger {id} sent to a neighbour of the aircraft cell, got {dest:?}"
        );
    }
    // Table order starts at S (octant 4): an empty map sends the first
    // passenger south.
    assert_eq!(out[0].3, Some((20, 21)), "first passenger scanned S first");

    for _ in 0..40 {
        fx.tick();
    }
    assert!(fx.cargo_ids(shad).is_empty());
    assert_eq!(fx.mission(shad), MissionId::from_known(MissionType::Guard));
}

/// The failure tail (`0x0073DC71`..`0x0073DC96`): a Gunner IFV whose only
/// passenger cannot leave (every neighbour carries the structural bridge
/// flag the state-3 scan skips at `0x0073D9EB`) re-adds the passenger and
/// re-applies the gunner weapon through `+0x4D4`, so the weapon override
/// the pop's `+0x4D8` cleared is back after every refused dispatch.
#[test]
fn ifv_keeps_gunner_weapon_when_every_exit_is_refused() {
    use crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL;

    let mut fx = Fixture::new(|_, _| false);
    {
        let terrain = fx.sim.resolved_terrain.as_mut().expect("terrain");
        for (dx, dy) in super::DIRECTION_DELTAS {
            let cell = terrain
                .cell_mut_for_test((20 + dx) as u16, (20 + dy) as u16)
                .expect("neighbour cell");
            cell.bridge_facts.raw_flags |= BRIDGE_FLAG_STRUCTURAL;
        }
    }
    let fv = fx.spawn("FV", 20, 20, 0);
    let pax = fx.board(fv, 1);
    fx.sim
        .substrate
        .entities
        .get_mut(fv)
        .expect("ifv")
        .set_gunner_weapon(1, fx.rules.object("FV").unwrap());
    assert!(fx.apply(Command::UnloadPassengers { transport_id: fv }));

    let mut reached_eject = false;
    for _ in 0..120 {
        fx.tick();
        let e = fx.sim.substrate.entities.get(fv).expect("ifv");
        if e.mission.current() == MissionId::from_known(MissionType::Unload)
            && e.mission.handler_state() == super::STATE_EJECT
        {
            reached_eject = true;
        }
        assert_eq!(
            (e.current_weapon_number(), e.current_turret_index()),
            (1, 2),
            "gunner weapon and turret retained across a refused ejection"
        );
    }
    assert!(reached_eject, "the handler reached state 3");
    assert!(!fx.revealed(pax[0]), "nobody left");
    assert_eq!(fx.cargo_ids(fv), vec![pax[0]]);
}

/// The relaxed pass with a vehicle passenger: every cell two steps out is
/// water, so no octant satisfies the strict "adjacent AND beyond" pair and
/// the restart at `i = 1` (`0x0073DA15`..`0x0073DA27`) accepts octant
/// `start + 1` alone. The passenger is placed on the
/// `Find_Nearby_Passable_Cell` result seeded at that exit cell (`0x0073DADD`,
/// stored over `[ESP+0x14]` at `0x0073DAE8`) and `Set_Destination` receives
/// the same slot (`0x0073DBF4`), not the cell beyond.
#[test]
fn relaxed_pass_drives_vehicle_passenger_to_the_fnpc_cell() {
    let ring_two = |x: u16, y: u16| (i32::from(x) - 20).abs().max((i32::from(y) - 20).abs()) == 2;
    let mut fx = Fixture::new(ring_two);
    let bfrt = fx.spawn("BFRT", 20, 20, 0x20);
    let bggy = fx.spawn("BGGY", 24, 24, 0);
    assert_eq!(
        fx.sim.techno_limbo_with_rules(bggy, &fx.rules, None),
        ConcealOutcome::Concealed
    );
    let cargo = fx
        .sim
        .substrate
        .entities
        .get_mut(bfrt)
        .and_then(|t| t.passenger_role.cargo_mut())
        .expect("transport cargo");
    assert!(cargo.can_accept(1.0), "cargo accepts the vehicle");
    cargo.board_forced_chain(&[(bggy, 1.0)]);
    fx.sim
        .substrate
        .entities
        .get_mut(bggy)
        .expect("passenger")
        .passenger_role = PassengerRole::inside_for_test(bfrt, true);
    // Already facing NE (0x20): state 0 keeps that heading, so the
    // state-3 scan starts at octant 5; the strict pass fails
    // everywhere (ring two is water) and the relaxed restart accepts octant
    // 6 (W), exit cell (19, 20). The FNPC expectation is taken now, with the
    // passenger still in limbo, exactly as the handler sees it: the free exit
    // cell passes every argument, so ring 0 answers it.
    let exit = (19u16, 20u16);
    let expected = exit;
    let spawn_elevation = fx
        .sim
        .substrate
        .entities
        .get(bggy)
        .unwrap()
        .barrel_elevation()
        .timer_start_frame();
    assert!(fx.apply(Command::UnloadPassengers { transport_id: bfrt }));

    let order = fx.run_until_revealed_with_missions(&[bggy], 200);
    assert_eq!(order.len(), 1, "the vehicle left: {order:?}");
    assert_eq!(order[0].2, MissionId::from_known(MissionType::Move));
    assert_eq!(fx.facing(bfrt), 0x20);
    assert_eq!(fx.cell(bggy), expected, "placed on the FNPC cell");
    assert_eq!(fx.facing(bggy), 0xC0, "octant 6 * 32");
    // Its departure Unlimbo levels the barrel again and re-aims it.
    let elevation = *fx
        .sim
        .substrate
        .entities
        .get(bggy)
        .unwrap()
        .barrel_elevation();
    assert!(elevation.timer_start_frame() > spawn_elevation);
    assert_eq!(elevation.destination(), 0x3800);
    let e = fx.sim.substrate.entities.get(bggy).expect("passenger");
    let dest = crate::sim::movement::movement_goal_cell(e).or(match e.navigation.nav_com() {
        Some(crate::sim::components::NavTargetRef::Cell { rx, ry }) => Some((rx, ry)),
        _ => None,
    });
    assert_ne!(
        dest,
        Some((18, 20)),
        "relaxed pass never targets the cell beyond"
    );
    assert!(
        dest.is_none_or(|d| d == expected),
        "Set_Destination got the FNPC slot, not the exit/beyond cell: {dest:?}"
    );
}

/// State 0's scorer (`0x00740B60`) calls a cell full of infantry only when
/// all five raw infantry bits are set (`+0x124 & 0x1F == 0x1F`), and it asks
/// only the nearest object's alliance. Three allied conscripts leave octant
/// 5's cell (19, 21) open, so from facing 0 the transport still backs onto
/// octant 5 and turns to octant 1 (0x20). Calling that cell closed would
/// pick octant 6 and turn to 0x40.
#[test]
fn exit_scorer_keeps_a_cell_with_three_allied_infantry_open() {
    let mut fx = Fixture::new(|_, _| false);
    for _ in 0..3 {
        let conscript = fx.spawn("E1", 19, 21, 0);
        assert!(fx.revealed(conscript));
    }
    let bfrt = fx.spawn("BFRT", 20, 20, 0);
    let pax = fx.board(bfrt, 1);
    assert!(fx.apply(Command::UnloadPassengers { transport_id: bfrt }));

    let order = fx.run_until_revealed(&pax, 200);
    assert_eq!(order.len(), 1, "the conscript left: {order:?}");
    assert_eq!(
        fx.sim
            .substrate
            .entities
            .get(bfrt)
            .unwrap()
            .body_facing
            .destination(),
        0x2000,
        "state 0 selects octant 5 even with three allied infantry there"
    );
}

/// Mission_Unload's scan asks the passenger's own `Can_Enter_Cell` at the
/// transport's height (`0x0073D994`, `0x0073D9C2`). Its height prelude
/// (`0x004D9C60`) refuses a cell two levels above the transport, so the strict
/// pass leaves octant 5's exit (19, 21) for octant 6's (19, 20).
#[test]
fn vehicle_unload_scan_refuses_an_exit_two_levels_up() {
    let mut fx = Fixture::new(|_, _| false);
    let terrain = fx.sim.resolved_terrain.as_mut().unwrap();
    terrain.cell_mut_for_test(19, 21).unwrap().level = 2;
    let bfrt = fx.spawn("BFRT", 20, 20, 0x20);
    let pax = fx.board(bfrt, 1);
    assert!(fx.apply(Command::UnloadPassengers { transport_id: bfrt }));

    let order = fx.run_until_revealed(&pax, 200);
    assert_eq!(order.len(), 1, "the conscript left: {order:?}");
    assert_eq!(fx.facing(bfrt), 0x20, "state 0 still backs onto octant 5");
    assert_eq!(fx.cell(pax[0]), (19, 20), "octant 6, not the raised exit");
}

/// Mission_Unload hands its Unlimbo the exit direction (`0x0073DB45..0x0073DB5B`).
/// After Foot success, UnitClass::Unlimbo snaps the turret (`+0x3A0`) to it
/// (`0x00737BBE..0x00737BD2`), and TechnoClass::Unlimbo the body (`0x006F6DAA`).
/// Same exit as `relaxed_pass_drives_vehicle_passenger_to_the_fnpc_cell`:
/// octant 6, direction 0xC0.
#[test]
fn vehicle_passenger_leaves_with_its_turret_on_the_exit_direction() {
    let ring_two = |x: u16, y: u16| (i32::from(x) - 20).abs().max((i32::from(y) - 20).abs()) == 2;
    let mut fx = Fixture::new(ring_two);
    fx.rules = rules_with_patch("[BGGY]\nTurret=yes\n");
    let bfrt = fx.spawn("BFRT", 20, 20, 0x20);
    let bggy = fx.spawn("BGGY", 24, 24, 0);
    assert_eq!(
        fx.sim.techno_limbo_with_rules(bggy, &fx.rules, None),
        ConcealOutcome::Concealed
    );
    let cargo = fx
        .sim
        .substrate
        .entities
        .get_mut(bfrt)
        .and_then(|t| t.passenger_role.cargo_mut())
        .expect("transport cargo");
    assert!(cargo.can_accept(1.0), "cargo accepts the vehicle");
    cargo.board_forced_chain(&[(bggy, 1.0)]);
    fx.sim
        .substrate
        .entities
        .get_mut(bggy)
        .expect("passenger")
        .passenger_role = PassengerRole::inside_for_test(bfrt, true);
    assert!(fx.apply(Command::UnloadPassengers { transport_id: bfrt }));

    let order = fx.run_until_revealed(&[bggy], 200);
    assert_eq!(order.len(), 1, "the vehicle left: {order:?}");
    assert_eq!(fx.cell(bggy), (19, 20));
    assert_eq!(fx.facing(bggy), 0xC0, "octant 6 * 32");
    let frame = fx.sim.session.binary_frame();
    let turret = fx
        .sim
        .substrate
        .entities
        .get(bggy)
        .and_then(|bggy| bggy.barrel_facing.as_ref())
        .expect("a Turret= unit has a turret facing")
        .current(frame);
    assert_eq!(turret, 0xC000, "the turret faces the exit too");
}

/// Both scan calls take the transport's height (`0x0073D97B`, `0x0073D9A9`),
/// not the exit's level. From a ramp at level 0, octant 5's exit (19, 21) is a
/// one-level step up the ramp admits. Its far cell (18, 22), also at level 1,
/// is one level above the transport's height behind a flat exit, so the strict
/// pass leaves octant 5 for octant 6's exit (19, 20).
#[test]
fn vehicle_unload_scan_judges_the_far_cell_from_the_transport_height() {
    let mut fx = Fixture::new(|_, _| false);
    let terrain = fx.sim.resolved_terrain.as_mut().unwrap();
    terrain.cell_mut_for_test(20, 20).unwrap().slope_type = 1;
    for (x, y) in [(19, 21), (18, 22)] {
        terrain.cell_mut_for_test(x, y).unwrap().level = 1;
    }
    let bfrt = fx.spawn("BFRT", 20, 20, 0x20);
    let pax = fx.board(bfrt, 1);
    assert!(fx.apply(Command::UnloadPassengers { transport_id: bfrt }));

    let order = fx.run_until_revealed(&pax, 200);
    assert_eq!(order.len(), 1, "the conscript left: {order:?}");
    assert_eq!(fx.cell(pax[0]), (19, 20), "octant 6, not the step up");
}

/// The relaxed pass accepts an exit on its own answer (`0x0073D9CC..0x0073D9DC`),
/// even when the cell two steps out has no coordinate. Water two cells from
/// (1, 20) refuses every strict candidate, and the relaxed pass starts at
/// octant 6, whose exit is (0, 20).
#[test]
fn relaxed_pass_takes_an_exit_whose_far_cell_is_off_the_map() {
    let ring_two = |x: u16, y: u16| (i32::from(x) - 1).abs().max((i32::from(y) - 20).abs()) == 2;
    let mut fx = Fixture::new(ring_two);
    let bfrt = fx.spawn("BFRT", 1, 20, 0x20);
    let pax = fx.board(bfrt, 1);
    assert!(fx.apply(Command::UnloadPassengers { transport_id: bfrt }));

    let order = fx.run_until_revealed(&pax, 200);
    assert_eq!(order.len(), 1, "the conscript left: {order:?}");
    assert_eq!(fx.cell(pax[0]), (0, 20));
}

/// A passenger that was in the playfield (`+0x3D5`) gets 7 from its
/// `Can_Enter_Cell` for a cell outside it (`0x0073F34C`); Mission_Unload raises
/// no `A8E7AC` until after the scan (`0x0073DA33`). In this playfield diamond
/// (`y - x < 2`) octant 5's exit (19, 21) and octant 6's far cell (18, 20) lie
/// outside, so the strict pass takes octant 7.
#[test]
fn vehicle_unload_scan_refuses_exits_outside_the_playfield() {
    let mut fx = Fixture::new(|_, _| false);
    fx.sim
        .set_playfield_bounds_for_test(Some(crate::map::playfield::PlayfieldBounds {
            base: 0,
            off_fc: -1,
            off_100: -100,
            off_104: 100,
            off_108: 200,
        }));
    let bfrt = fx.spawn("BFRT", 20, 20, 0);
    let pax = fx.board(bfrt, 1);
    fx.sim
        .substrate
        .entities
        .get_mut(pax[0])
        .expect("passenger")
        .set_in_playfield(true);
    assert!(fx.apply(Command::UnloadPassengers { transport_id: bfrt }));

    let order = fx.run_until_revealed(&pax, 200);
    assert_eq!(order.len(), 1, "the conscript left: {order:?}");
    assert_eq!(fx.cell(pax[0]), (19, 19), "octant 7's exit");
}

/// The vehicle passenger's Find_Nearby_Passable_Cell (`0x0073DA96..0x0073DADD`)
/// runs the playfield anchor gate with no height gate. The passenger never
/// entered the playfield (`+0x3D5` clear, as a reinforcement's cargo), so its
/// `Can_Enter_Cell` does not refuse cells outside it (`0x0073F34C`), and the
/// strict pass takes octant 5, exit (19, 21), which lies outside this playfield
/// diamond (`y - x < 2`). Ring 0 fails and ring 1 answers. Its only in-diamond
/// cells besides the transport's own are (19, 20) and (20, 21), raised two
/// levels.
#[test]
fn vehicle_passenger_off_the_playfield_exit_lands_on_raised_in_playfield_ground() {
    let mut fx = Fixture::new(|_, _| false);
    fx.sim
        .set_playfield_bounds_for_test(Some(crate::map::playfield::PlayfieldBounds {
            base: 0,
            off_fc: -1,
            off_100: -100,
            off_104: 100,
            off_108: 200,
        }));
    let terrain = fx.sim.resolved_terrain.as_mut().unwrap();
    for (x, y) in [(19, 20), (20, 21)] {
        terrain.cell_mut_for_test(x, y).unwrap().level = 2;
    }
    let bfrt = fx.spawn("BFRT", 20, 20, 0);
    let bggy = fx.spawn("BGGY", 24, 24, 0);
    assert_eq!(
        fx.sim.techno_limbo_with_rules(bggy, &fx.rules, None),
        ConcealOutcome::Concealed
    );
    let cargo = fx
        .sim
        .substrate
        .entities
        .get_mut(bfrt)
        .and_then(|t| t.passenger_role.cargo_mut())
        .expect("transport cargo");
    assert!(cargo.can_accept(1.0), "cargo accepts the vehicle");
    cargo.board_forced_chain(&[(bggy, 1.0)]);
    fx.sim
        .substrate
        .entities
        .get_mut(bggy)
        .expect("passenger")
        .passenger_role = PassengerRole::inside_for_test(bfrt, true);
    fx.sim
        .substrate
        .entities
        .get_mut(bggy)
        .expect("passenger")
        .set_in_playfield(false);
    assert!(fx.apply(Command::UnloadPassengers { transport_id: bfrt }));

    let order = fx.run_until_revealed_with_missions(&[bggy], 200);
    assert_eq!(order.len(), 1, "the vehicle left: {order:?}");
    let cell = fx.cell(bggy);
    assert!(
        [(19, 20), (20, 21)].contains(&cell),
        "placed on raised in-playfield ground, not the exit: {cell:?}"
    );
}

fn set_altitude(fx: &mut Fixture, id: u64, altitude: i32) {
    fx.sim.set_object_height_unmarked(id, altitude);
}

fn handler_state(fx: &Fixture, id: u64) -> u32 {
    fx.sim
        .substrate
        .entities
        .get(id)
        .expect("entity")
        .mission
        .handler_state()
}

/// The NavCom-less state graph of `0x004151E0`: a flying Nighthawk ordered to
/// unload takes state 0 → 2 (`0x0041530C`, one epilogue draw at
/// `0x0041525A`), then state 2 polls with `return 1` (`0x004154B1`) and draws
/// NO Scenario RNG while it descends; once landed it moves to state 3
/// (`0x004154A4`, still no draw) and the ejection dispatch draws again.
#[test]
fn descending_nighthawk_draws_no_scenario_rng_until_it_ejects() {
    let mut fx = Fixture::new(|_, _| false);
    let shad = fx.spawn("SHAD", 20, 20, 0);
    let pax = fx.board(shad, 2);
    set_altitude(&mut fx, shad, 600);
    assert!(fx.apply(Command::UnloadPassengers { transport_id: shad }));

    // First dispatch: state 0 → 2 with exactly one epilogue draw.
    let before = fx.sim.scenario_rng.state();
    crate::sim::aircraft::dispatch_mission(&mut fx.sim, shad, &fx.rules, Default::default());
    fx.sim
        .session
        .set_binary_frame_for_test(fx.sim.session.binary_frame() + 1);
    assert_eq!(handler_state(&fx, shad), super::AIR_STATE_WAIT_STOP);
    assert_ne!(
        fx.sim.scenario_rng.state(),
        before,
        "state-0 epilogue draws once"
    );

    // Descend 20 leptons per frame: 30 airborne dispatches, zero draws.
    let mut altitude = 600;
    let mut airborne_draws = 0;
    while altitude > 0 {
        altitude -= 20;
        set_altitude(&mut fx, shad, altitude);
        let before = fx.sim.scenario_rng.state();
        crate::sim::aircraft::dispatch_mission(&mut fx.sim, shad, &fx.rules, Default::default());
        fx.sim
            .session
            .set_binary_frame_for_test(fx.sim.session.binary_frame() + 1);
        if fx.sim.scenario_rng.state() != before {
            airborne_draws += 1;
        }
        if altitude > 0 {
            assert_eq!(handler_state(&fx, shad), super::AIR_STATE_WAIT_STOP);
            assert_eq!(fx.cargo_ids(shad).len(), 2, "nothing leaves mid-air");
        }
    }
    assert_eq!(airborne_draws, 0, "state 2 polls without an epilogue draw");
    // The landed poll moved to state 3 with `return 1`: the next frame ejects.
    assert_eq!(handler_state(&fx, shad), super::AIR_STATE_EJECT);
    assert_eq!(fx.cargo_ids(shad).len(), 2);
    let before = fx.sim.scenario_rng.state();
    crate::sim::aircraft::dispatch_mission(&mut fx.sim, shad, &fx.rules, Default::default());
    assert!(
        fx.revealed(pax[1]),
        "cargo head ejected on the state-3 dispatch"
    );
    assert_ne!(
        fx.sim.scenario_rng.state(),
        before,
        "state-3 epilogue draws"
    );
}

/// `UnitClass::Mission_Unload` state 0 waits (`return 10`) only while the
/// locomotor's Is_Moving (`0x0073D729`) is true: a Drive with a destination
/// holds the unload before it has any speed.
#[test]
fn unit_unload_waits_on_the_locomotors_is_moving() {
    let run = |setup: &dyn Fn(&mut crate::sim::game_entity::GameEntity)| {
        let mut fx = Fixture::new(|_, _| false);
        let bfrt = fx.spawn("BFRT", 20, 20, 0x40);
        fx.board(bfrt, 1);
        setup(fx.sim.substrate.entities.get_mut(bfrt).expect("transport"));
        let delay = super::unit_mission_unload(
            &mut fx.sim,
            &fx.rules,
            bfrt,
            None,
            crate::sim::world::FrameEffects::default(),
        );
        (delay, handler_state(&fx, bfrt))
    };
    let waiting = (super::WAIT_MOVING_FRAMES, super::STATE_PICK_EXIT);
    assert_ne!(run(&|_| {}), waiting, "a standing transport unloads");
    assert_eq!(
        run(&|e| {
            {
                let loco = e.locomotor.as_mut().unwrap();
                assert!(loco.ensure_installed_track_state());
                assert!(loco.store_track_destination(
                    crate::sim::movement::track_process::TrackFamily::Drive,
                    Some(crate::sim::components::DriveCoord::cell(25, 20, 0))
                ));
            };
            e.foot_speed.set_speed_fraction(SIM_ZERO);
        }),
        waiting,
        "a Drive with a destination waits before it has any speed"
    );
}

/// A Jumpjet transport (retail `[SHAD]` and `[HIND]` are Jumpjet
/// `[VehicleTypes]` entries) waits at state 0 on the locomotor's moving byte
/// (`+0x48`, Is_Moving `0x0054AE50`), not on Is_Moving_Now's state
/// (`0x0054D0D0`, false in the hold): holding in the air with the byte set,
/// it waits.
#[test]
fn jumpjet_transport_unload_waits_on_the_moving_byte_in_the_hold() {
    let run = |moving: bool| {
        let mut fx = Fixture::new(|_, _| false);
        let hind = fx.spawn("HIND", 20, 20, 0x40);
        fx.board(hind, 1);
        set_altitude(&mut fx, hind, 500);
        let runtime = fx
            .sim
            .substrate
            .entities
            .get_mut(hind)
            .and_then(|e| e.locomotor.as_mut())
            .and_then(|loco| loco.jumpjet_runtime_mut())
            .expect("Jumpjet runtime");
        *runtime = runtime
            .clone()
            .with_phase_for_test(crate::sim::movement::jumpjet_movement::jumpjet_flight::STATE_HOLD)
            .with_moving_for_test(moving);
        let delay = super::unit_mission_unload(
            &mut fx.sim,
            &fx.rules,
            hind,
            None,
            crate::sim::world::FrameEffects::default(),
        );
        (delay, handler_state(&fx, hind))
    };
    let waiting = (super::WAIT_MOVING_FRAMES, super::STATE_PICK_EXIT);
    assert_eq!(run(true), waiting, "holding with the moving byte set waits");
    assert_ne!(run(false), waiting, "holding with it clear does not");
}

/// A Jumpjet transport ordered to unload in flight lands first: the order's
/// NULL destination reaches the Unit setter while a NavCom is set
/// (`0x00741A80`), whose Foot tail runs Jumpjet `Stop_Moving` (`0x0054B4D0`):
/// a `Move_To` the floor of the nearest passable cell. The moving byte holds
/// state 0 until touchdown clears it.
#[test]
fn jumpjet_transport_ordered_to_unload_in_flight_lands_first() {
    let mut fx = Fixture::new(|_, _| false);
    let hind = fx.spawn("HIND", 8, 20, 0x40);
    let pax = fx.board(hind, 2);
    assert!(fx.apply(Command::Move {
        entity_id: hind,
        target_rx: 36,
        target_ry: 20,
    }));
    let altitude = |fx: &Fixture| {
        crate::sim::movement::air_movement::current_fly_height(
            fx.sim.substrate.entities.get(hind).expect("aircraft"),
            fx.sim.resolved_terrain.as_ref(),
        )
    };
    for _ in 0..150 {
        fx.tick();
    }
    let ordered_at = fx.cell(hind);
    assert!(altitude(&fx) > 0, "in flight");
    assert!(fx.apply(Command::UnloadPassengers { transport_id: hind }));
    let mut landed_reveals = 0;
    for _ in 0..600 {
        fx.tick();
        let out = pax.iter().filter(|&&id| fx.revealed(id)).count();
        if out > landed_reveals {
            assert_eq!(altitude(&fx), 0, "a passenger left in the air");
            landed_reveals = out;
        }
        if out == pax.len() {
            break;
        }
    }
    let unloaded_at = fx.cell(hind);
    assert_eq!(landed_reveals, pax.len());
    assert!(
        unloaded_at.0 <= ordered_at.0 + 2,
        "unloaded at {unloaded_at:?}, ordered at {ordered_at:?}"
    );
}

/// The Unload order is a MEGAMISSION whose class destination and target
/// setters run with NULL (`0x004C747C`, `0x004C7467`): a driving transport
/// stops at its committed head and unloads there, not where it was sent.
#[test]
fn unload_order_stops_a_driving_transport_where_it_is() {
    let mut fx = Fixture::new(|_, _| false);
    let bfrt = fx.spawn("BFRT", 6, 20, 0x40);
    let pax = fx.board(bfrt, 2);
    assert!(fx.apply(Command::Move {
        entity_id: bfrt,
        target_rx: 36,
        target_ry: 20,
    }));
    for _ in 0..40 {
        fx.tick();
    }
    let ordered_at = fx.cell(bfrt);
    assert!(
        ordered_at.0 > 6 && ordered_at.0 < 30,
        "under way at {ordered_at:?}"
    );
    assert!(fx.apply(Command::UnloadPassengers { transport_id: bfrt }));
    let transport = fx.sim.substrate.entities.get(bfrt).expect("transport");
    assert!(
        transport.navigation.nav_com().is_none(),
        "the order nulls the NavCom"
    );
    assert_eq!(fx.run_until_revealed(&pax, 400).len(), 2);
    let unloaded_at = fx.cell(bfrt);
    assert!(
        unloaded_at.0 <= ordered_at.0 + 2,
        "unloaded at {unloaded_at:?}, ordered at {ordered_at:?}"
    );
}

/// `AircraftClass::Mission_Unload` state 2 moves on to state 3 when the
/// locomotor's Is_Moving (`0x0041549D`) is false.
/// The fixture's `[SHAD]` is an aircraft to reach this slot.
#[test]
fn landed_aircraft_transport_unload_asks_the_locomotor() {
    let run = |setup: &dyn Fn(&mut crate::sim::game_entity::GameEntity)| {
        let mut fx = Fixture::new(|_, _| false);
        let shad = fx.spawn("SHAD", 20, 20, 0);
        fx.board(shad, 2);
        set_altitude(&mut fx, shad, 600);
        assert!(fx.apply(Command::UnloadPassengers { transport_id: shad }));
        crate::sim::aircraft::dispatch_mission(&mut fx.sim, shad, &fx.rules, Default::default());
        fx.sim
            .session
            .set_binary_frame_for_test(fx.sim.session.binary_frame() + 1);
        assert_eq!(handler_state(&fx, shad), super::AIR_STATE_WAIT_STOP);
        set_altitude(&mut fx, shad, 0);
        setup(fx.sim.substrate.entities.get_mut(shad).expect("aircraft"));
        // State 2 polls once its dispatch timer is due.
        for _ in 0..120 {
            crate::sim::aircraft::dispatch_mission(
                &mut fx.sim,
                shad,
                &fx.rules,
                Default::default(),
            );
            fx.sim
                .session
                .set_binary_frame_for_test(fx.sim.session.binary_frame() + 1);
            if handler_state(&fx, shad) != super::AIR_STATE_WAIT_STOP {
                break;
            }
        }
        handler_state(&fx, shad)
    };
    assert_eq!(
        run(&|_| {}),
        super::AIR_STATE_EJECT,
        "a still landed aircraft transport ejects"
    );
    assert_eq!(
        run(&|e| {
            let runtime = e
                .locomotor
                .as_mut()
                .and_then(|loco| loco.jumpjet_runtime_mut())
                .expect("Jumpjet runtime");
            *runtime = runtime.clone().with_moving_for_test(true);
        }),
        super::AIR_STATE_WAIT_STOP,
        "a moving Jumpjet keeps it waiting"
    );
}

/// VERA-internal bounded escape (native `0x00415511`/`0x0041553E` loses the
/// passenger): with the aircraft cell already holding three infantry, the
/// first ejection fails, the passenger stays in the hold, the aircraft leaves
/// Unload for Guard, and the handler draws nothing more.
#[test]
fn nighthawk_failed_ejection_keeps_cargo_and_leaves_for_guard() {
    let mut fx = Fixture::new(|_, _| false);
    let blockers: Vec<u64> = (0..3).map(|_| fx.spawn("E1", 20, 20, 0)).collect();
    let spots: Vec<Option<u8>> = blockers
        .iter()
        .map(|&id| {
            fx.sim
                .substrate
                .entities
                .get(id)
                .expect("blocker")
                .sub_cell()
        })
        .collect();
    assert_eq!(
        spots.iter().flatten().count(),
        3,
        "three functional sub-cells taken: {spots:?}"
    );
    let shad = fx.spawn("SHAD", 20, 20, 0);
    let pax = fx.board(shad, 2);
    // LIFO hold: the last boarder is the head (`AddPassenger @ 0x004733A0`
    // prepends). The refused ejection must leave exactly this order.
    let held = fx.cargo_ids(shad);
    assert_eq!(held, pax.iter().rev().copied().collect::<Vec<_>>());
    set_altitude(&mut fx, shad, 0);
    assert!(fx.apply(Command::UnloadPassengers { transport_id: shad }));

    // State 0 → 3 (landed, team-less), one draw.
    crate::sim::aircraft::dispatch_mission(&mut fx.sim, shad, &fx.rules, Default::default());
    assert_eq!(handler_state(&fx, shad), super::AIR_STATE_EJECT);
    // Advance to the state-3 dispatch: the ejection is refused, and the idle
    // mode's tail queues Guard and commences it when ready (`0x00417B44`).
    for _ in 0..20 {
        fx.sim
            .session
            .set_binary_frame_for_test(fx.sim.session.binary_frame() + 1);
        crate::sim::aircraft::dispatch_mission(&mut fx.sim, shad, &fx.rules, Default::default());
        if fx.mission(shad) == MissionId::from_known(MissionType::Guard) {
            break;
        }
    }
    assert_eq!(
        fx.mission(shad),
        MissionId::from_known(MissionType::Guard),
        "one failed attempt leaves for Guard"
    );
    assert_eq!(fx.mission_queued(shad), MissionId::NONE);
    assert_eq!(
        fx.cargo_ids(shad),
        held,
        "hold order retained after the refusal"
    );
    assert!(
        pax.iter().all(|&id| !fx.revealed(id)),
        "nobody left the hold"
    );

    // The handler stays silent.
    let before = fx.sim.scenario_rng.state();
    for _ in 0..60 {
        fx.sim
            .session
            .set_binary_frame_for_test(fx.sim.session.binary_frame() + 1);
        crate::sim::aircraft::dispatch_mission(&mut fx.sim, shad, &fx.rules, Default::default());
    }
    assert_eq!(
        fx.sim.scenario_rng.state(),
        before,
        "no further Unload draws"
    );
    assert_eq!(fx.cargo_ids(shad), held, "cargo still retained");
}

/// KickOutUnit raises no A8E7AC (`0x00415B10..0x00415C50`), so the
/// passenger's Unlimbo runs Object's class +1AC query (`0x005F4F1B`) after
/// PlaceInfantryInCell found him a free spot: an enemy infantryman standing
/// in the landed aircraft's cell refuses him. The refusal takes the bounded
/// escape above.
#[test]
fn nighthawk_passenger_is_refused_by_an_enemy_in_the_aircraft_cell() {
    use crate::sim::house_state::HouseState;

    let mut fx = Fixture::new(|_, _| false);
    let owners: Vec<_> = [OWNER, "Russians"]
        .into_iter()
        .map(|name| {
            let owner = fx.sim.interner.intern(name);
            fx.sim
                .houses
                .register(HouseState::new(owner, 0, None, true, 0, 10));
            owner
        })
        .collect();
    fx.sim.houses.reorder_for_test(&owners);
    fx.sim
        .session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    let enemy = fx
        .sim
        .spawn_object("E1", "Russians", 20, 20, 0, &fx.rules)
        .expect("enemy conscript");
    assert!(fx.revealed(enemy));
    let shad = fx.spawn("SHAD", 20, 20, 0);
    let pax = fx.board(shad, 1);
    set_altitude(&mut fx, shad, 0);
    assert!(fx.apply(Command::UnloadPassengers { transport_id: shad }));

    for _ in 0..40 {
        fx.sim
            .session
            .set_binary_frame_for_test(fx.sim.session.binary_frame() + 1);
        crate::sim::aircraft::dispatch_mission(&mut fx.sim, shad, &fx.rules, Default::default());
        if fx.mission(shad) == MissionId::from_known(MissionType::Guard) {
            break;
        }
    }
    assert_eq!(
        fx.mission(shad),
        MissionId::from_known(MissionType::Guard),
        "the refused ejection leaves for Guard"
    );
    assert_eq!(fx.cargo_ids(shad), pax, "the passenger stays in the hold");
    assert!(!fx.revealed(pax[0]));
}

#[test]
fn cargo_departure_ground_reveal_rejection_preserves_route_retry_state() {
    use crate::sim::movement::locomotor::MovementLayer;
    for aircraft in [false, true] {
        for already_revealed in [false, true] {
            let mut fx = Fixture::new(|_, _| false);
            let transport = fx.spawn(if aircraft { "SHAD" } else { "FV" }, 20, 20, 0);
            let passenger = fx.board(transport, 1)[0];
            let peer = fx.spawn("E1", 30, 30, 0);
            if already_revealed {
                assert!(matches!(
                    fx.sim.reveal(passenger),
                    crate::sim::world::RevealOutcome::Revealed { .. }
                ));
            } else {
                // Deliberately inconsistent cargo fixture: reach actual Reveal's
                // early marked-object rejection after the caller clears Inside.
                fx.sim
                    .substrate
                    .entities
                    .get_mut(passenger)
                    .unwrap()
                    .lifecycle
                    .set_cell_marked_for_test(true);
            }
            fx.sim
                .substrate
                .entities
                .get_mut(peer)
                .unwrap()
                .mark_live_contact_with(passenger);
            {
                let carrier = fx.sim.substrate.entities.get_mut(transport).unwrap();
                carrier.set_gunner_selection_for_test(99, -1);
                let cargo = carrier.passenger_role.cargo_mut().unwrap();
                // Admission-time size differs from current E1 Size=1.
                let mut sizes = cargo.passenger_sizes().to_vec();
                sizes[0] = 7.0;
                cargo.set_passenger_sizes_for_test(sizes);
            }
            let held = serde_json::to_value(
                fx.sim
                    .substrate
                    .entities
                    .get(transport)
                    .unwrap()
                    .passenger_role
                    .cargo(),
            )
            .unwrap();
            let rng_before = fx.sim.scenario_rng.state();
            if aircraft {
                assert!(!super::eject_from_aircraft(
                    &mut fx.sim,
                    &fx.rules,
                    None,
                    transport,
                    crate::sim::world::FrameEffects::default()
                ));
            } else {
                assert!(matches!(
                    super::eject_head_passenger(
                        &mut fx.sim,
                        &fx.rules,
                        transport,
                        None,
                        crate::sim::world::FrameEffects::default()
                    ),
                    super::EjectOutcome::Failed
                ));
            }
            let carrier = fx.sim.substrate.entities.get(transport).unwrap();
            assert_eq!(
                serde_json::to_value(carrier.passenger_role.cargo()).unwrap(),
                held
            );
            assert_eq!(
                (
                    carrier.current_weapon_number(),
                    carrier.current_turret_index()
                ),
                if aircraft { (99, -1) } else { (1, 2) }
            );
            let pax = fx.sim.substrate.entities.get(passenger).unwrap();
            assert_eq!(pax.passenger_role.inside_transport_id(), Some(transport));
            assert!(pax.lifecycle.in_limbo());
            assert_eq!(pax.lifecycle.cell_marked(), !already_revealed);
            assert!(!pax.in_logic_vector.is_member());
            assert_eq!(
                pax.locomotor.as_ref().unwrap().layer(),
                MovementLayer::Ground
            );
            assert_eq!(
                fx.sim
                    .substrate
                    .entities
                    .get(peer)
                    .unwrap()
                    .has_live_contact_with(passenger),
                !already_revealed
            );
            assert!(
                fx.sim
                    .sound_events
                    .iter()
                    .all(|event| !matches!(event, SimSoundEvent::LeaveTransport { .. }))
            );
            // Real placement still consumed its draw before Reveal rejected.
            assert_ne!(fx.sim.scenario_rng.state(), rng_before);
            println!(
                "CARGO_TRACE ground {aircraft} {already_revealed} {:?} {:?} {:?}",
                fx.sim.scenario_rng.state(),
                held,
                pax.position
            );
        }
    }
}

/// Original 73D630/736990 controls: state 1 reads Unit+6AF, independently
/// of whether the body FacingClass is still rotating. The latch here is the
/// supplied result of that separate Facing_Update owner.
#[test]
fn turning_unload_uses_the_turret_latch_and_native_cadence() {
    use crate::sim::movement::FacingClass;
    let native = crate::test_fixture::json("tools/spatial_oracle/transport_unload_gate.json");
    for row in native.as_array().unwrap() {
        let turret_latch = row["latch"].as_bool().unwrap();
        let hull_turning = row["hull_rotating"].as_bool().unwrap();
        let kind = if row["turret"].as_bool().unwrap() {
            "FV"
        } else {
            "BFRT"
        };
        let state = row["state"].as_u64().unwrap() as u32;
        let mut fx = Fixture::new(|_, _| false);
        let carrier = fx.spawn(kind, 20, 20, 0);
        fx.board(carrier, 2);
        fx.sim.session.set_binary_frame_for_test(100);
        fx.sim
            .substrate
            .mission_assign_exact(carrier, MissionId::from_known(MissionType::Unload), 100)
            .unwrap();
        let entity = fx.sim.substrate.entities.get_mut(carrier).unwrap();
        entity.body_facing = FacingClass::new(0, 5);
        if hull_turning {
            entity.body_facing.set(0x2000, 100);
        }
        entity.turret_rotation_latch.set_for_test(turret_latch);
        entity.mission.set_handler_state(super::STATE_TURNING);
        fx.sim.session.set_binary_frame_for_test(101);
        assert_eq!(
            fx.sim
                .substrate
                .entities
                .get(carrier)
                .unwrap()
                .body_facing
                .is_rotating(101),
            hull_turning
        );
        fx.sim.scenario_rng = crate::sim::rng::SimRng::from_native_state_hex_for_test(
            row["rng_before"].as_str().unwrap(),
        );
        let delay = super::unit_mission_unload(
            &mut fx.sim,
            &fx.rules,
            carrier,
            None,
            crate::sim::world::FrameEffects::default(),
        );
        assert_eq!(
            fx.sim
                .substrate
                .entities
                .get(carrier)
                .unwrap()
                .mission
                .handler_state(),
            state,
            "{kind}: hull_turning={hull_turning}, turret_latch={turret_latch}"
        );
        assert_eq!(delay, row["delay"].as_i64().unwrap() as i32);
        assert_eq!(
            fx.sim.scenario_rng.native_state_hex(),
            row["rng_after"].as_str().unwrap(),
            "one epilogue draw only while waiting on the turret"
        );
    }
}

/// Stock BFRT/E1 rules and ART, through the player command and ordinary
/// object turns. A turretless carrier must eject before its hull settles.
#[test]
fn retail_bfrt_ejects_while_turning_and_resumes_after_save() {
    let native = crate::test_fixture::json("tools/spatial_oracle/transport_unload_retail.json");
    let native = &native["bfrt_turn"];
    let ejection = native["calls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row.get("ejection_heading").is_some())
        .unwrap();
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    let mut fx = Fixture::new(|_, _| false);
    fx.rules = retail.rules;
    fx.sim.session.set_binary_frame_for_test(100);
    fx.sim.scenario_rng = crate::sim::rng::SimRng::new(1);
    let carrier = fx.spawn("BFRT", 20, 20, 0);
    let passengers = fx.board(carrier, 5);
    assert!(
        fx.sim
            .substrate
            .entities
            .get(carrier)
            .unwrap()
            .barrel_facing
            .is_none()
    );
    assert!(fx.apply(Command::UnloadPassengers {
        transport_id: carrier
    }));

    for _ in 0..10 {
        fx.tick();
        if fx
            .sim
            .substrate
            .entities
            .get(carrier)
            .unwrap()
            .mission
            .handler_state()
            == super::STATE_TURNING
        {
            break;
        }
    }
    let entity = fx.sim.substrate.entities.get(carrier).unwrap();
    assert_eq!(entity.mission.handler_state(), super::STATE_TURNING);
    assert!(entity.body_facing.is_rotating(fx.frame()));
    let bytes = crate::sim::snapshot::GameSnapshot::save_validated(&fx.sim, 17, 23, "unload", 0);
    let mut restored = crate::sim::snapshot::GameSnapshot::load_validated(&bytes, 17, 23, "")
        .unwrap()
        .sim;
    restored.retain_process_state_for_load(crate::sim::world::SnapshotProcessState::Live(&fx.sim));
    restored.resolved_terrain = fx.sim.resolved_terrain.clone();
    restored.restore_after_snapshot_load().unwrap();
    restored.rebuild_caches_after_load(
        fx.sim.resolved_terrain.as_ref().unwrap().clone(),
        fx.sim.terrain_speed_config().clone(),
        &fx.rules,
    );
    assert!(restored.rebuild_dynamic_navigation(&fx.rules));
    // Native ScenarioLoad resets the Scenario RNG to Seed(0). Apply that
    // same load prior to the uninterrupted control before comparing futures.
    fx.sim.scenario_rng = crate::sim::rng::SimRng::new(0);
    // Foot Load also clears the move-sound countdown, including idle Feet.
    for id in fx.sim.substrate.entities.keys_sorted() {
        fx.sim.substrate.restore_move_sound_state_after_load(id);
    }
    assert_eq!(
        restored.state_hash(),
        fx.sim.state_hash(),
        "restored {:?}; uninterrupted {:?}",
        restored.hash_owner_diagnostics_for_test(),
        fx.sim.hash_owner_diagnostics_for_test()
    );

    let mut order = Vec::new();
    for _ in 0..160 {
        fx.tick();
        restored.advance_tick(&[], Some(&fx.rules), Some(&fx.grid), None, 66);
        assert_eq!(
            restored.state_hash(),
            fx.sim.state_hash(),
            "frame {}",
            fx.frame()
        );
        for &passenger in &passengers {
            if fx.revealed(passenger) && !order.iter().any(|&(id, _)| id == passenger) {
                if order.is_empty() {
                    let entity = fx.sim.substrate.entities.get(carrier).unwrap();
                    assert!(
                        entity.body_facing.is_rotating(fx.frame()),
                        "turretless BFRT must eject at its mid-turn heading (frame {})",
                        fx.frame()
                    );
                    assert!(!entity.turret_rotation_latch.is_set());
                    // advance_tick has advanced the public clock. Compare
                    // the heading actually consumed during the object turn.
                    assert_eq!(
                        u64::from(entity.body_facing_current(fx.frame() - 1)),
                        ejection["ejection_heading"].as_u64().unwrap()
                    );
                    let position = ejection["passenger_position"].as_array().unwrap();
                    let source = native["source_position"].as_array().unwrap();
                    let offset = |axis: usize| {
                        position[axis].as_i64().unwrap() / 256
                            - source[axis].as_i64().unwrap() / 256
                    };
                    assert_eq!(
                        fx.cell(passenger),
                        ((20 + offset(0)) as u16, (20 + offset(1)) as u16),
                        "native mid-turn exit cell relative to the carrier"
                    );
                    assert_eq!(
                        fx.sim
                            .substrate
                            .entities
                            .get(passenger)
                            .unwrap()
                            .passenger_role
                            .inside_transport_id()
                            .is_none(),
                        ejection["transporter_cleared"].as_bool().unwrap(),
                        "departure clears the native Transporter link"
                    );
                }
                assert_eq!(
                    fx.mission(passenger),
                    MissionId::from_known(MissionType::Move)
                );
                order.push((passenger, fx.frame()));
            }
        }
        if order.len() == passengers.len()
            && fx.mission(carrier) == MissionId::from_known(MissionType::Guard)
        {
            break;
        }
    }
    assert_eq!(
        order.iter().map(|&(id, _)| id).collect::<Vec<_>>(),
        passengers.iter().rev().copied().collect::<Vec<_>>()
    );
    for pair in order.windows(2) {
        assert!((14..=16).contains(&(pair[1].1 - pair[0].1)));
    }
    assert!(fx.cargo_ids(carrier).is_empty());
    assert_eq!(
        fx.mission(carrier),
        MissionId::from_known(MissionType::Guard)
    );
}

/// Aligned hull controls: real Facing_Update must hold the IFV and Flak
/// Track for their turret arcs, while the turretless BFRT can proceed.
#[test]
fn retail_aligned_transports_wait_only_for_their_turrets() {
    let native = crate::test_fixture::json("tools/spatial_oracle/transport_unload_retail.json");
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    for case in ["bfrt_aligned", "fv_turret", "htk_turret"] {
        let control = &native[case];
        let kind = control["kind"].as_str().unwrap();
        let native_waited = control["calls"].as_array().unwrap().iter().any(|row| {
            row["state_before"] == 1 && row["state"] == 1 && row["latch_before"].as_bool().unwrap()
        });
        let mut fx = Fixture::new(|_, _| false);
        fx.rules = retail.rules.clone();
        fx.sim.session.set_binary_frame_for_test(100);
        let carrier = fx.spawn(
            kind,
            20,
            20,
            (control["initial_facing"].as_u64().unwrap() >> 8) as u8,
        );
        let passenger = fx.board(carrier, 1)[0];
        let entity = fx.sim.substrate.entities.get_mut(carrier).unwrap();
        if let Some(barrel) = entity.barrel_facing.as_mut() {
            barrel.set(0x6000, 100);
        }
        assert!(fx.apply(Command::UnloadPassengers {
            transport_id: carrier
        }));
        let mut waited_on_turret = false;
        for _ in 0..80 {
            fx.tick();
            let entity = fx.sim.substrate.entities.get(carrier).unwrap();
            assert!(
                !entity.body_facing.is_rotating(fx.frame()),
                "{kind}: aligned hull"
            );
            if entity.mission.handler_state() == super::STATE_TURNING
                && entity.turret_rotation_latch.is_set()
            {
                waited_on_turret = true;
                assert!(
                    !fx.revealed(passenger),
                    "{kind}: rotating turret holds passenger"
                );
            }
            if fx.revealed(passenger) {
                break;
            }
        }
        assert_eq!(waited_on_turret, native_waited, "{kind}");
        assert!(fx.revealed(passenger), "{kind}: eventually ejects");
        assert!(fx.cargo_ids(carrier).is_empty(), "{kind}: clears cargo");
        assert_eq!(
            fx.mission(passenger),
            MissionId::from_known(MissionType::Move)
        );
    }
}
