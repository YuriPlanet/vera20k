//! Fly Process (`0x004CCB40`) frame by frame, its map-edge admission and
//! Stop_Moving (`0x004CCFD0`) against the original executable:
//! `tools/spatial_oracle/fly_process.json`, `fly_map_edge.json` and
//! `fly_stop.json`.

use super::{FlyMapEdge, fly_map_edge};
use crate::map::entities::EntityCategory;
use crate::map::playfield::PlayfieldBounds;
use crate::rules::art_data::ArtRegistry;
use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
use crate::sim::ammo::TechnoAmmo;
use crate::sim::combat::{Health, TargetKind};
use crate::sim::components::{DriveCoord, NavTargetRef};
use crate::sim::game_entity::GameEntity;
use crate::sim::mission::state::MissionTestFixture;
use crate::sim::mission::{MissionDispatchTimer, MissionId};
use crate::sim::movement::FacingClass;
use crate::sim::movement::fly_height::FlyRuntime;
use crate::sim::movement::ground_pose;
use crate::sim::movement::locomotor::LocomotorState;
use crate::sim::rng::SimRng;
use crate::sim::world::Simulation;
use crate::sim::world::lifecycle_tests::{insert_entity, install_common_raw_terrain};
use crate::util::fixed_math::SimFixed;

const AIRCRAFT: u64 = 1;
const TARGET: u64 = 2;
const DOCK: u64 = 3;

fn int(input: &serde_json::Value, key: &str, default: i64) -> i64 {
    input[key].as_i64().unwrap_or(default)
}

fn flag(input: &serde_json::Value, key: &str, default: bool) -> bool {
    input[key].as_bool().unwrap_or(default)
}

fn ints<const N: usize>(value: &serde_json::Value, default: [i32; N]) -> [i32; N] {
    value.as_array().map_or(default, |values| {
        std::array::from_fn(|i| values[i].as_i64().unwrap() as i32)
    })
}

fn coord([x, y, z]: [i32; 3]) -> DriveCoord {
    DriveCoord { x, y, z }
}

/// The oracle's dock: a 3x2 Helipad with four DockingOffsets unless the row
/// says otherwise.
fn dock_offsets(dock: &serde_json::Value) -> Vec<[i32; 3]> {
    dock["offsets"].as_array().map_or_else(
        || vec![[0, -128, 0], [0, 128, 0], [-256, -128, 0], [-256, 128, 0]],
        |offsets| offsets.iter().map(|o| ints(o, [0; 3])).collect(),
    )
}

/// The oracle's BuildingTypes by index: a row's `docks` and buildings name
/// them so.
const BUILDING_TYPES: [&str; 2] = ["PAD", "DEPOT"];

/// The row's types: `TEST` (Speed through the production reader, `Dock=`
/// from the row's `docks`), the `VICTIM` its Target is, the `PAD` its dock
/// is and a `DEPOT`. A Projectile ROT of 0 makes the weapon strafe
/// (`0x0041B7F0`). Clear ground costs Track and Foot 1.0, as the oracle's
/// land cost table.
fn rules_for(input: &serde_json::Value) -> RuleSet {
    let dock = &input["dock"];
    let offsets = dock_offsets(dock);
    let [width, height] = ints(&dock["foundation"], [3, 2]);
    let docks: Vec<&str> = input["docks"].as_array().map_or_else(Vec::new, |docks| {
        docks
            .iter()
            .map(|index| BUILDING_TYPES[index.as_u64().unwrap() as usize])
            .collect()
    });
    let mut rules = RuleSet::from_ini(&IniFile::from_str(&format!(
        "[General]\nFlightLevel=1500\n[AudioVisual]\nPoseDir={}\n\
         [AircraftTypes]\n0=TEST\n[VehicleTypes]\n0=VICTIM\n\
         [BuildingTypes]\n0=PAD\n1=DEPOT\n\
         [TEST]\nStrength={}\nSpeed={}\nFlightLevel={}\nSlowdownDistance={}\n\
         HunterSeeker={}\nIsDropship={}\nFlyBy={}\nFighter={}\nLandable={}\n\
         AirportBound={}\nPitchAngle=0\nAmmo=1\nPrimary=TestGun\nDock={}\n\
         Locomotor={{4A582746-9839-11D1-B709-00A024DDAFD1}}\n\
         [TestGun]\nDamage=10\nRange=5\nROF=10\nProjectile=TestShot\nWarhead=TestWH\n\
         [TestShot]\nROT={}\n[TestWH]\nVerses=100%\n[VICTIM]\nStrength=100\n\
         [PAD]\nHelipad={}\nUnitRepair={}\nNumberOfDocks={}\n[DEPOT]\nStrength=500\n\
         [Clear]\nTrack=100%\nFoot=100%\n",
        int(input, "pose_dir", 2),
        int(input, "strength", 150),
        int(input, "ini_speed", 14),
        int(input, "flight_level", -1),
        int(input, "slowdown", 500),
        flag(input, "hunter_seeker", false),
        flag(input, "dropship", false),
        flag(input, "fly_by", false),
        flag(input, "fighter", false),
        flag(input, "landable", true),
        flag(input, "airport_bound", false),
        docks.join(","),
        if flag(input, "strafe", false) { 0 } else { 3 },
        flag(dock, "helipad", true),
        flag(dock, "unit_repair", false),
        offsets.len(),
    )))
    .unwrap();
    rules.general.blockage_path_delay_ticks = 11;
    let mut art = format!("[PAD]\nFoundation={width}x{height}\n");
    for (index, [x, y, z]) in offsets.iter().enumerate() {
        art += &format!("DockingOffset{index}={x},{y},{z}\n");
    }
    rules.install_art_data(ArtRegistry::from_ini(&IniFile::from_str(&art)));
    rules
}

/// One living aircraft in the row's in-flight state at its first frame, as
/// `fly_process.py` builds it: over flat ground of 128x128 cells with MapSize
/// 64x64 and LocalSize 0,0,64,64 (or the row's), registered in the air tracker and Display
/// at its height; a Target and a dock building where the row has them.
fn fixture(input: &serde_json::Value) -> (Simulation, RuleSet) {
    let rules = rules_for(input);
    let frame = int(input, "frame", 1000) as u32;
    let mut sim = Simulation::with_seed(0);
    sim.session.set_map_width_for_test(128);
    sim.session.set_map_height_for_test(128);
    let [left, top, local_width, local_height] = ints(&input["local"], [0, 0, 64, 64]);
    sim.playfield_bounds = Some(PlayfieldBounds::from_normalized_local_size(
        64,
        left,
        top,
        local_width,
        local_height,
    ));
    sim.playfield_size_height = Some(64);
    install_common_raw_terrain(&mut sim, 128, 128, 0, None);
    sim.overlay_grid = Some(crate::sim::overlay_grid::OverlayGrid::new(128, 128));
    sim.scenario_rng = SimRng::new(31);
    sim.session.set_binary_frame_for_test(frame);
    let [x, y] = ints(&input["cell"], [60, 64]);
    assert_eq!(sim.substrate.allocate_stable_id(), AIRCRAFT);
    insert_entity(&mut sim, AIRCRAFT, EntityCategory::Aircraft);
    let entity = sim.substrate.entities.get_mut(AIRCRAFT).unwrap();
    entity.lifecycle.set_object_alive_for_test(true);
    entity.lifecycle.set_in_limbo_for_test(false);
    entity.position.set_rx_for_test(x as u16);
    entity.position.set_ry_for_test(y as u16);
    entity.position.set_sub_x_for_test(SimFixed::from_num(128));
    entity.position.set_sub_y_for_test(SimFixed::from_num(128));
    entity
        .position
        .set_exact_z_leptons_for_test(Some(int(input, "z", 1500) as i32));
    entity.health.set_for_test(int(input, "health", 150) as i32);
    let facing = int(input, "facing", 0x4000) as u16;
    let rot = int(input, "rot", 3) as i32;
    entity.body_facing = FacingClass::new(facing, rot);
    entity.barrel_facing = Some(FacingClass::new(facing, rot));
    entity.mission.apply_test_fixture(MissionTestFixture {
        current: MissionId::from_raw(int(input, "mission", 2) as i32),
        suspended: MissionId::NONE,
        queued: MissionId::from_raw(int(input, "queued", -1) as i32),
        movement_bypass_latch: 0,
        handler_state: 0,
        mission_start_frame: 0,
        ai_counter: 0,
        dispatch_timer: MissionDispatchTimer::at_frame(frame),
    });
    let mut ammo = TechnoAmmo::new(1);
    ammo.set_current_for_test(int(input, "ammo", 1) as i32);
    entity.install_ammo_for_test(Some(ammo));
    if input["target"].is_array() {
        entity
            .attack_target
            .set_for_test(Some(TargetKind::Entity(TARGET)));
    }
    entity
        .navigation
        .set_nav_com_for_test(match &input["nav_com"] {
            serde_json::Value::String(kind) if kind == "dock" => {
                Some(NavTargetRef::Object { id: DOCK })
            }
            serde_json::Value::Bool(true) => Some(NavTargetRef::cell(80, 64)),
            _ => None,
        });
    if flag(input, "loaner", false) {
        entity.mission_only.mark();
    }
    entity.set_in_playfield(flag(input, "in_playfield", true));
    entity
        .mission_leaf
        .set_aircraft_action_latch(flag(input, "locked", false));
    entity
        .mission_leaf
        .set_aircraft_transition_ready(u8::from(flag(input, "ready", false)));
    entity.locomotor = Some(LocomotorState::from_object_type(
        rules.object("TEST").unwrap(),
        0,
    ));
    let destination = ints(&input["destination"], [80 * 256 + 128, 64 * 256 + 128, 0]);
    let fly = entity
        .locomotor
        .as_mut()
        .unwrap()
        .fly_runtime_mut()
        .unwrap();
    *fly = serde_json::from_value::<FlyRuntime>(serde_json::json!({
        "target_height": int(input, "target_height", 1500),
        "taking_off": flag(input, "taking_off", false),
        "landing": flag(input, "landing", false),
        "destination": destination,
        "cruise_mode": flag(input, "cruise", false),
        "moving": flag(input, "moving", true),
        "airport_bound": flag(input, "airport_bound", false),
    }))
    .unwrap();
    fly.set_target_speed_for_test(SimFixed::from_num(
        input["target_speed"].as_f64().unwrap_or(1.0),
    ));
    fly.set_current_speed_for_test(SimFixed::from_num(input["speed"].as_f64().unwrap_or(1.0)));
    Simulation::aircraft_tracker_add(&sim.session, &mut sim.substrate, AIRCRAFT);
    sim.add_entity_occupancy(AIRCRAFT);
    sim.submit_entity_display(AIRCRAFT, Some(&rules), None);

    let owner = sim.substrate.entities.get(AIRCRAFT).unwrap().owner();
    if input["target"].is_array() {
        let [tx, ty, tz] = ints(&input["target"], [0; 3]);
        let mut target = GameEntity::new_at_frame_zero_for_test(
            TARGET,
            (tx / 256) as u16,
            (ty / 256) as u16,
            0,
            0,
            sim.interner.intern("Russians"),
            Health::new(100),
            sim.interner.intern("VICTIM"),
            EntityCategory::Unit,
            0,
            5,
            true,
        );
        target.lifecycle.set_object_alive_for_test(true);
        target
            .position
            .set_sub_x_for_test(SimFixed::from_num(tx % 256));
        target
            .position
            .set_sub_y_for_test(SimFixed::from_num(ty % 256));
        target.position.set_exact_z_leptons_for_test(Some(tz));
        sim.substrate.entities.insert(target);
    }
    let dock = &input["dock"];
    if dock.is_object() {
        let [bx, by] = ints(&dock["origin"], [0, 0]);
        let [width, height] = ints(&dock["foundation"], [3, 2]);
        let mut building = GameEntity::new_at_frame_zero_for_test(
            DOCK,
            bx as u16,
            by as u16,
            0,
            0,
            owner,
            Health::new(1000),
            sim.interner.intern("PAD"),
            EntityCategory::Structure,
            0,
            5,
            true,
        );
        building.lifecycle.set_object_alive_for_test(true);
        building
            .type_facts
            .set_foundation_for_test(format!("{width}x{height}"));
        building.position.set_exact_z_leptons_for_test(Some(0));
        building
            .radio_contacts
            .set_capacity(dock_offsets(dock).len());
        if let Some(slot) = dock["slot"].as_u64() {
            building.radio_contacts.set_slot(slot as usize, AIRCRAFT);
            sim.substrate
                .entities
                .get_mut(AIRCRAFT)
                .unwrap()
                .radio_contacts
                .insert(DOCK);
        }
        sim.substrate.entities.insert(building);
        sim.add_entity_occupancy(DOCK);
    }
    (sim, rules)
}

/// A Stop row's world as `fly_stop.py` builds it: the Process fixture with
/// the row's pitch and Location, its Units (`technos`, not Marked) and its
/// buildings (Marked unless in Limbo), in the oracle's ObjectClass order
/// (aircraft, Units, buildings), the buildings also the House's list in row
/// order.
fn stop_fixture(input: &serde_json::Value) -> (Simulation, RuleSet) {
    let (mut sim, rules) = fixture(input);
    let owner = sim.substrate.entities.get(AIRCRAFT).unwrap().owner();
    let enemy = sim.interner.intern("Russians");
    let entity = sim.substrate.entities.get_mut(AIRCRAFT).unwrap();
    if let Some(pitch) = input["pitch"].as_f64() {
        entity.flight_attitude =
            serde_json::from_value(serde_json::json!({ "pitch": SimFixed::from_num(pitch) }))
                .unwrap();
    }
    if input["location"].is_array() {
        let [x, y, z] = ints(&input["location"], [0; 3]);
        entity
            .position
            .set_rx_for_test((x.div_euclid(256)).max(0) as u16);
        entity
            .position
            .set_ry_for_test((y.div_euclid(256)).max(0) as u16);
        entity.position.set_sub_x_for_test(SimFixed::from_num(
            x - i32::from(entity.position.rx()) * 256,
        ));
        entity.position.set_sub_y_for_test(SimFixed::from_num(
            y - i32::from(entity.position.ry()) * 256,
        ));
        entity.position.set_exact_z_leptons_for_test(Some(z));
    }
    let empty = Vec::new();
    for (n, techno) in input["technos"]
        .as_array()
        .unwrap_or(&empty)
        .iter()
        .enumerate()
    {
        let [x, y, z] = ints(&techno["xyz"], [0; 3]);
        let mut unit = GameEntity::new_at_frame_zero_for_test(
            10 + n as u64,
            (x / 256) as u16,
            (y / 256) as u16,
            0,
            0,
            if flag(techno, "enemy", false) {
                enemy
            } else {
                owner
            },
            Health::new(100),
            sim.interner.intern("VICTIM"),
            EntityCategory::Unit,
            0,
            5,
            true,
        );
        unit.lifecycle.set_object_alive_for_test(true);
        unit.lifecycle
            .set_in_limbo_for_test(flag(techno, "limbo", false));
        unit.position
            .set_sub_x_for_test(SimFixed::from_num(x % 256));
        unit.position
            .set_sub_y_for_test(SimFixed::from_num(y % 256));
        unit.position.set_exact_z_leptons_for_test(Some(z));
        sim.substrate.entities.insert(unit);
    }
    let mut list = Vec::new();
    for (n, building) in input["buildings"]
        .as_array()
        .unwrap_or(&empty)
        .iter()
        .enumerate()
    {
        let id = 20 + n as u64;
        let [bx, by] = ints(&building["origin"], [0, 0]);
        let [width, height] = ints(&building["foundation"], [2, 2]);
        let kind = BUILDING_TYPES[int(building, "type", 0) as usize];
        let mut structure = GameEntity::new_at_frame_zero_for_test(
            id,
            bx as u16,
            by as u16,
            0,
            0,
            if flag(building, "enemy", false) {
                enemy
            } else {
                owner
            },
            Health::new(1000),
            sim.interner.intern(kind),
            EntityCategory::Structure,
            0,
            5,
            true,
        );
        structure.lifecycle.set_object_alive_for_test(true);
        structure
            .type_facts
            .set_foundation_for_test(format!("{width}x{height}"));
        structure.position.set_exact_z_leptons_for_test(Some(0));
        sim.substrate.entities.insert(structure);
        if flag(building, "limbo", false) {
            sim.substrate
                .entities
                .get_mut(id)
                .unwrap()
                .lifecycle
                .set_in_limbo_for_test(true);
        } else {
            sim.add_entity_occupancy(id);
        }
        list.push(id);
    }
    sim.houses
        .get_or_register_for_test(owner, || {
            crate::sim::house_state::HouseState::new(owner, 0, None, true, 0, 1)
        })
        .base_projection_mut()
        .replace_buildings_for_test(list);
    (sim, rules)
}

/// Where a frame differs from native, for the failure message. `idle_calls`
/// counts the frame's Enter_Idle_Mode calls, which both sides record.
fn frame_mismatch(
    sim: &Simulation,
    frame: u32,
    expected: &serde_json::Value,
    idle_calls: u32,
) -> Option<String> {
    let entity = sim.substrate.entities.get(AIRCRAFT)?;
    let fly = entity.locomotor.as_ref()?.fly_runtime()?;
    let xyz = ground_pose::object_location(entity, sim.resolved_terrain.as_ref());
    let secondary = entity.barrel_facing.as_ref()?;
    let destination = fly.destination();
    let actual = serde_json::json!({
        "xyz": [xyz.x, xyz.y, xyz.z],
        "primary": [entity.body_facing.current(frame), entity.body_facing.destination()],
        "secondary": [secondary.current(frame), secondary.destination()],
        "flight_level": fly.target_height(),
        "destination": [destination.x, destination.y, destination.z],
        "moving": u8::from(fly.moving()),
        "phase": [
            u8::from(fly.taking_off()),
            u8::from(fly.landing()),
            u8::from(fly.landing_effect_latched()),
        ],
        "cruise": u8::from(fly.cruise_mode()),
        "mission": [entity.mission.current().raw(), entity.mission.queued().raw()],
        "locked": entity.mission_leaf.as_aircraft()?.action_latch(),
        "in_playfield": u8::from(entity.in_playfield()),
    });
    for (key, value) in actual.as_object()? {
        if &expected[key] != value {
            return Some(format!("{key}: {value} vs native {}", expected[key]));
        }
    }
    // The fractions are binary64 natively. VERA's SimFixed ramp step is
    // 6554/65536, 0.4 quanta above 0.1: at most ten steps separate two clamps
    // to a target, which with the start value's rounding keeps the current
    // speed within 4.5 quanta. The coordinates above compare exactly.
    for (key, value) in [
        ("target_speed", fly.target_speed()),
        ("speed", fly.current_speed()),
    ] {
        let native = expected[key].as_f64()?;
        if (value.to_num::<f64>() - native).abs() > 4.5 / 65536.0 {
            return Some(format!("{key}: {value} vs native {native}"));
        }
    }
    // Begin_Landing's refusal calls Enter_Idle_Mode(0, 1), which the oracle
    // records.
    let refused = expected["calls"]
        .as_array()?
        .iter()
        .filter(|call| call[0] == "enter_idle_mode")
        .count();
    (refused != idle_calls as usize)
        .then(|| format!("Enter_Idle_Mode calls: {idle_calls} vs native {refused}"))
}

/// Every frame of every row of `fly_process.json` through the production
/// object-turn entry (`tick_air_movement_with_cell_lists_one`): the
/// aircraft's coordinates, both facings, speeds, flight level, destination,
/// moving byte, phase flags, cruise mode, mission, lock, playfield latch and
/// Begin_Landing's refusal, and the Scenario RNG after the last frame.
#[test]
fn fly_process_matches_native_frames() {
    let oracle: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/fly_process.json",
    ))
    .unwrap();
    let rows = oracle["process"].as_array().unwrap();
    assert_eq!(rows.len(), 25);
    let mut failures = Vec::new();
    for row in rows {
        let input = &row["input"];
        let name = input["name"].as_str().unwrap();
        let (mut sim, rules) = fixture(input);
        let start = sim.session.binary_frame();
        let frames = row["frames"].as_array().unwrap();
        let mut failed = false;
        for (n, expected) in frames.iter().enumerate() {
            let frame = start + n as u32;
            sim.session.set_binary_frame_for_test(frame);
            let ((), idle_calls) = crate::sim::aircraft::record_idle_calls_for_test(|| {
                sim.tick_air_movement_with_cell_lists_one(
                    AIRCRAFT,
                    Some(&rules),
                    None,
                    crate::sim::world::FrameEffects::default(),
                )
                .unwrap();
            });
            if let Some(mismatch) = frame_mismatch(&sim, frame, expected, idle_calls) {
                failures.push(format!("{name} frame {n}: {mismatch}"));
                failed = true;
                break;
            }
        }
        if !failed && u64::from(sim.scenario_rng.next_u32()) != row["next_random"].as_u64().unwrap()
        {
            failures.push(format!("{name}: Scenario RNG diverged"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Every row of `fly_map_edge.json`: In_Bounds, the push toward the middle
/// column (LocalSize X, `0x00565660`), the scatter's one Scenario draw, the
/// skipped SetLocation, and the stream after it. vt+0x4DC is given native's
/// answer, which the push shows: [`fly_map_edge_gate_matches_native_rows`]
/// compares VERA's.
#[test]
fn fly_map_edge_matches_native_rows() {
    let rows: Vec<serde_json::Value> = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/fly_map_edge.json",
    ))
    .unwrap();
    assert_eq!(rows.len(), 78);
    for row in &rows {
        let input = &row["input"];
        let name = input["name"].as_str().unwrap();
        let [width, height] = ints(&input["size"], [64, 64]);
        let [left, top, local_width, local_height] = ints(&input["local"], [2, 2, 60, 56]);
        let edge = FlyMapEdge {
            width,
            height,
            bounds: PlayfieldBounds::from_normalized_local_size(
                width,
                left,
                top,
                local_width,
                local_height,
            ),
            span: int(input, "span_12c", 127) as i32,
        };
        let calls: Vec<&str> = row["calls"]
            .as_array()
            .unwrap()
            .iter()
            .map(|call| call.as_str().unwrap())
            .collect();
        let fly_by = flag(input, "fly_by", false);
        let may_leave = calls.contains(&"to_local") || fly_by;
        let candidate = coord(ints(&input["proposed"], [128 * 256, 64 * 256, 1500]));
        let mut rng = SimRng::new(int(input, "seed", 31) as u64);
        let (placed, committed) = fly_map_edge(candidate, &edge, may_leave, fly_by, &mut rng);
        assert_eq!(committed, row["commit"].as_bool().unwrap(), "{name}");
        assert_eq!(
            [placed.x, placed.y, placed.z],
            ints(&row["candidate"], [0; 3]),
            "{name}"
        );
        if let Some(local) = row["local_coords"].as_array().and_then(|l| l.first()) {
            let cell = ((candidate.x / 256) as i16, (candidate.y / 256) as i16);
            let (x, y) = crate::map::playfield::cell_to_local_packed(edge.bounds, cell);
            assert_eq!([i32::from(x), i32::from(y)], ints(local, [0; 2]), "{name}");
        }
        assert_eq!(
            u64::from(rng.next_u32()),
            row["next_random"].as_u64().unwrap(),
            "{name}"
        );
    }
}

/// vt+0x4DC (`AircraftClass @ 0x0041B890`) where `fly_map_edge.json` shows
/// native's answer: the push follows a true one, a placement without it a
/// false one. Team rows ask `0x006EC300`, whose waypoint read VERA does not
/// make (`aircraft_may_leave_map`'s RESIDUAL); they are left out.
#[test]
fn fly_map_edge_gate_matches_native_rows() {
    let rows: Vec<serde_json::Value> = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/fly_map_edge.json",
    ))
    .unwrap();
    let mut compared = 0;
    for row in &rows {
        let input = &row["input"];
        let calls = row["calls"].as_array().unwrap();
        if !calls.iter().any(|call| call == "aircraft_gate")
            || input["team"].is_object()
            || flag(input, "fly_by", false)
        {
            continue;
        }
        let native = calls.iter().any(|call| call == "to_local");
        let mut sim = Simulation::with_seed(0);
        insert_entity(&mut sim, AIRCRAFT, EntityCategory::Aircraft);
        let entity = sim.substrate.entities.get_mut(AIRCRAFT).unwrap();
        entity.mission.apply_test_fixture(MissionTestFixture {
            current: MissionId::from_raw(int(input, "mission", 5) as i32),
            suspended: MissionId::NONE,
            queued: MissionId::from_raw(int(input, "queued_mission", -1) as i32),
            movement_bypass_latch: 0,
            handler_state: 0,
            mission_start_frame: 0,
            ai_counter: 0,
            dispatch_timer: MissionDispatchTimer::at_frame(0),
        });
        if flag(input, "mission_only", true) {
            entity.mission_only.mark();
        }
        entity.set_in_playfield(flag(input, "in_playfield", true));
        if flag(input, "target", false) {
            entity
                .attack_target
                .set_for_test(Some(TargetKind::Entity(TARGET)));
            let mut target = GameEntity::new_at_frame_zero_for_test(
                TARGET,
                64,
                64,
                0,
                0,
                sim.interner.intern("Russians"),
                Health::new(100),
                sim.interner.intern("VICTIM"),
                EntityCategory::Unit,
                0,
                5,
                true,
            );
            target.lifecycle.set_object_alive_for_test(true);
            sim.substrate.entities.insert(target);
        }
        assert_eq!(
            sim.aircraft_may_leave_map(AIRCRAFT),
            native,
            "{}",
            input["name"]
        );
        compared += 1;
    }
    assert_eq!(compared, 46);
}

/// Every row of `fly_stop.json`: what Stop_Moving (`0x004CCFD0`) does
/// ([`Simulation::fly_stop_order`]) against the call native made in place
/// of its stubbed class setter or ReceiveDamage, and the Scenario RNG after
/// it. The airfield search's zone calls all ask MovementZone Normal without
/// bridge resolution, as VERA's does.
#[test]
fn fly_stop_matches_native_rows() {
    use crate::sim::world::fly_orders::FlyStopOrder;
    let oracle: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/fly_stop.json",
    ))
    .unwrap();
    let rows: Vec<_> = oracle["stop"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["input"]["stun"].as_bool() != Some(true))
        .collect();
    assert_eq!(rows.len(), 30);
    let mut failures = Vec::new();
    for row in rows {
        let input = &row["input"];
        let name = input["name"].as_str().unwrap();
        for zone in row["zones"].as_array().unwrap() {
            assert_eq!(
                (&zone[2], &zone[3]),
                (&serde_json::json!(0), &serde_json::json!(0))
            );
        }
        let (mut sim, rules) = stop_fixture(input);
        let order = sim.fly_stop_order(AIRCRAFT, &rules);
        let native = row["calls"].as_array().unwrap();
        let expected = match native.first() {
            None => None,
            Some(call) if call[0] == "receive_damage" => Some(FlyStopOrder::SelfDestruct),
            Some(call) => {
                let [x, y] = [call[1][1].as_i64().unwrap(), call[1][2].as_i64().unwrap()];
                Some(FlyStopOrder::Destination(Some(NavTargetRef::cell(
                    x as u16, y as u16,
                ))))
            }
        };
        if order != expected {
            failures.push(format!("{name}: {order:?} vs native {expected:?}"));
        }
        if u64::from(sim.scenario_rng.next_u32()) != row["next_random"].as_u64().unwrap() {
            failures.push(format!("{name}: Scenario RNG diverged"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The death arm's Stun (`FootClass::Stun @ 0x004D5660`, then
/// `TechnoClass::Stun @ 0x006FCD40`) on the Stop rows' aircraft at Health 0:
/// the re-targets Fly's Stop_Moving makes through both NULL destinations and
/// Stop_Driver, in order (none for an Attack aircraft's first NULL while it
/// holds its TarCom), and the Scenario draws Find_Attack_Cell takes around
/// an occupied cell each time.
#[test]
fn fly_death_stun_matches_native_rows() {
    use crate::sim::world::LifecycleTestEvent;
    use crate::sim::world::fly_orders::FlyStopOrder;
    let oracle: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/fly_stop.json",
    ))
    .unwrap();
    let rows: Vec<_> = oracle["stop"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["input"]["stun"].as_bool() == Some(true))
        .collect();
    assert_eq!(rows.len(), 6);
    let cell = |at: &serde_json::Value| {
        FlyStopOrder::Destination(Some(NavTargetRef::cell(
            at[1].as_u64().unwrap() as u16,
            at[2].as_u64().unwrap() as u16,
        )))
    };
    let mut failures = Vec::new();
    for row in rows {
        let input = &row["input"];
        let name = input["name"].as_str().unwrap();
        let (mut sim, rules) = stop_fixture(input);
        let entity = sim.substrate.entities.get_mut(AIRCRAFT).unwrap();
        entity.health.set_for_test(0);
        if let Some(n) = input["target"].as_u64() {
            entity
                .attack_target
                .set_for_test(Some(TargetKind::Entity(10 + n)));
        }
        sim.clear_lifecycle_test_events_for_test();
        sim.techno_death_stun(
            AIRCRAFT,
            crate::sim::world::UninitContext::new(Some(&rules), None),
        );
        let orders: Vec<_> = sim
            .lifecycle_test_events_for_test()
            .iter()
            .filter_map(|event| match event {
                LifecycleTestEvent::FlyStopOrdered {
                    id: AIRCRAFT,
                    order,
                } => Some(*order),
                _ => None,
            })
            .collect();
        let native: Vec<_> = row["calls"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|call| call[0] == "assign_destination" && !call[1].is_null())
            .map(|call| cell(&call[1]))
            .collect();
        if orders != native {
            failures.push(format!("{name}: {orders:?} vs native {native:?}"));
        }
        if u64::from(sim.scenario_rng.next_u32()) != row["next_random"].as_u64().unwrap() {
            failures.push(format!("{name}: Scenario RNG diverged"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
