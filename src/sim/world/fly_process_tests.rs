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
use crate::util::native_x87::NativeF64Bits;

const AIRCRAFT: u64 = 1;
const TARGET: u64 = 2;
const DOCK: u64 = 3;

/// Original grounded ORCA MoveTo4CCC80 -> Process4CCB40 -> speed4CFE20.
/// The native nonzero ramp pays 13 at its fourth step and reaches 35 at11.
#[test]
fn retail_harrier_move_preserves_native_acceleration_across_save() {
    use crate::sim::command::{Command, CommandEnvelope};
    use crate::sim::snapshot::GameSnapshot;
    let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules_with_art() else {
        return;
    };
    let mut sim = crate::sim::arena_fixture::rival_arena_with_size(&rules, 17, 128);
    let owner = sim.intern("Americans");
    sim.session.set_current_house_for_test(Some(owner));
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::Skirmish);
    sim.set_playfield_bounds_for_test(Some(crate::sim::cell_rect::PlayfieldBounds {
        base: 64,
        ..crate::sim::arena_fixture::OPEN_PLAYFIELD
    }));
    sim.set_playfield_size_height_for_test(Some(64));
    sim.set_input_delay_ticks(0);
    for (kind, house, cell) in [
        ("GAPOWR", "Americans", (40, 48)),
        ("NAPOWR", "Russians", (90, 80)),
    ] {
        sim.spawn_object(kind, house, cell.0, cell.1, 0, &rules)
            .unwrap();
    }
    let plane = sim
        .spawn_object("ORCA", "Americans", 50, 64, 0, &rules)
        .unwrap();
    let type_speed = crate::util::fixed_math::ra2_speed_to_leptons_per_frame(
        rules.object("ORCA").unwrap().speed,
    );
    assert_eq!(type_speed, 35);
    let mut commands = vec![CommandEnvelope::new(
        owner,
        sim.session.tick(),
        Command::Move {
            entity_id: plane,
            target_rx: 100,
            target_ry: 64,
        },
    )];
    let native_paid = [3, 7, 10, 13, 17, 20, 24, 27, 31, 34, 35];
    let mut step = 0;
    let mut physical_step = 0;
    let mut restored = None;
    for _ in 0..160 {
        let frame = sim.session.binary_frame();
        let before = sim.entities().get(plane).unwrap();
        let before_xyz = ground_pose::object_location(before, sim.resolved_terrain.as_ref());
        let before_facing = before.body_facing.current(frame);
        let before_fly = before.locomotor.as_ref().unwrap().fly_runtime().unwrap();
        let before_paid = crate::sim::movement::air_movement::current_fly_speed(
            type_speed,
            before_fly.current_speed(),
        );
        if before_paid > 0 {
            assert!(!before_fly.has_phase_callback());
            assert!(before_xyz.z > 0);
            assert!(before_fly.destination().x - before_xyz.x > 0x300);
            assert_eq!(
                before_paid,
                native_paid[physical_step],
                "native pre-motion payment{}",
                physical_step + 1
            );
        }
        let grid = sim.path_grid_snapshot();
        sim.advance_tick(&commands, Some(&rules), grid.as_deref(), None, 33);
        if let Some(restored) = restored.as_mut() {
            let restored: &mut Simulation = restored;
            let grid = restored.path_grid_snapshot();
            restored.advance_tick(&commands, Some(&rules), grid.as_deref(), None, 33);
            let continued = sim.entities().get(plane).unwrap();
            let loaded = restored.entities().get(plane).unwrap();
            assert_eq!(
                ground_pose::object_location(loaded, restored.resolved_terrain.as_ref()),
                ground_pose::object_location(continued, sim.resolved_terrain.as_ref())
            );
            assert_eq!(
                loaded.locomotor.as_ref().unwrap().fly_runtime(),
                continued.locomotor.as_ref().unwrap().fly_runtime()
            );
        }
        commands = sim.take_due_commands();
        let plane = sim.entities().get(plane).expect("moving Harrier survives");
        if before_paid > 0 {
            // Motion pays the retained pre-tick current before the late ramp.
            // Use the independently recorded native budget with the live
            // facing; the full-heading trig has its separate native corpus.
            let expected = crate::util::native_trig::facing_step_world_xy(
                [before_xyz.x, before_xyz.y],
                before_facing,
                native_paid[physical_step],
            );
            let actual = ground_pose::object_location(plane, sim.resolved_terrain.as_ref());
            assert_eq!(
                [actual.x, actual.y],
                expected,
                "admitted native movement{} facing{before_facing:04x}",
                physical_step + 1
            );
            physical_step += 1;
        }
        let speed = plane
            .locomotor
            .as_ref()
            .unwrap()
            .fly_runtime()
            .unwrap()
            .current_speed();
        let paid = crate::sim::movement::air_movement::current_fly_speed(type_speed, speed);
        if step == 0 && paid == 0 {
            continue;
        }
        if step < native_paid.len() {
            assert_eq!(paid, native_paid[step], "native ramp step{}", step + 1);
            step += 1;
        }
        if step == 3 && restored.is_none() {
            // Save the live, non-dyadic acceleration state without changing it.
            let bytes = GameSnapshot::save(&sim, 0, 0, "Fly acceleration", 0);
            let mut loaded = GameSnapshot::load(&bytes).unwrap().sim;
            loaded
                .retain_process_state_for_load(crate::sim::world::SnapshotProcessState::Live(&sim));
            loaded.resolved_terrain = sim.resolved_terrain.clone();
            loaded.restore_after_snapshot_load().unwrap();
            let loaded_plane = loaded.entities().get(plane.stable_id()).unwrap();
            assert_eq!(
                ground_pose::object_location(loaded_plane, loaded.resolved_terrain.as_ref()),
                ground_pose::object_location(plane, sim.resolved_terrain.as_ref())
            );
            assert_eq!(
                loaded
                    .entities()
                    .get(plane.stable_id())
                    .unwrap()
                    .locomotor
                    .as_ref()
                    .unwrap()
                    .fly_runtime(),
                plane.locomotor.as_ref().unwrap().fly_runtime()
            );
            restored = Some(loaded);
        }
        if step == native_paid.len() && physical_step == native_paid.len() {
            return;
        }
    }
    panic!("ordinary Move must produce the complete nonzero speed ramp");
}

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
    // The original Process fixture packs binary64 priors directly; these
    // inputs do not carry the Q16 restriction of the paid-step corpus.
    fly.set_target_speed_bits_for_test(NativeF64Bits::from_bits(
        input["target_speed"].as_f64().unwrap_or(1.0).to_bits(),
    ));
    fly.set_current_speed_bits_for_test(NativeF64Bits::from_bits(
        input["speed"].as_f64().unwrap_or(1.0).to_bits(),
    ));
    Simulation::aircraft_tracker_add(&sim.session, &mut sim.substrate, AIRCRAFT);
    sim.add_entity_occupancy(AIRCRAFT);
    Simulation::submit_entity_display(
        &sim.interner,
        &mut sim.substrate,
        &sim.resolved_terrain,
        AIRCRAFT,
        &rules,
        None,
    );

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
            .set_foundation_for_test(format!("{width}x{height}").into());
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
            .set_foundation_for_test(format!("{width}x{height}").into());
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
    // Python's saved decimal values carry the original binary64 stores.
    for (key, value) in [
        ("target_speed", fly.target_speed()),
        ("speed", fly.current_speed()),
    ] {
        let native = expected[key].as_f64()?;
        if value.bits() != native.to_bits() {
            return Some(format!(
                "{key}: {:016x} vs native {:016x} ({native})",
                value.bits(),
                native.to_bits()
            ));
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
        // The oracle is stock gamemd. Under Ares a `HunterSeeker=` Fly takes
        // the Ares flight instead: Process (1004D7A0, hook 0x4CCB84),
        // UpdateTarget (1004DA90, hook 0x4CD9C8), Horizontal_Step (1004DC30,
        // hook 0x4CF3D0) and the climb (1004D930/1004DA20, hooks 0x4CDE64/
        // 0x4CDF54), which `sim::hunter_seeker` and `fly_height` test.
        if crate::rules::ares_switch::ares_way() && flag(input, "hunter_seeker", false) {
            continue;
        }
        // Phobos `AircraftExt::GetLandingDir` (0x4CF31C) replaces the
        // stock landing direction. With a dock link and the default
        // `AircraftDockingDir.DefaultToPoseDir=yes` the pose dir replaces
        // the contact's facing, so these stock rows no longer apply; the
        // Phobos branch is covered by
        // `landing_base::tests::get_landing_dir_follows_the_native_branch_order`.
        if input["dock"].is_object() {
            continue;
        }
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
                    &rules,
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
        sim.stun(AIRCRAFT, crate::sim::world::UninitContext::new(&rules));
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

/// A grounded aircraft returning to its pad, for the two Phobos hooks: type
/// `RETURN` with `ExtendedAircraftMissions.<key>=<value>`, its dock `+0x6CC`
/// and NavCom (`Foot+0x5A4`) both the pad. `pad_extra` appends rules keys to
/// the `[PAD]` section.
fn phobos_returning_aircraft(
    key: &str,
    value: bool,
    docks: i32,
    pad_extra: &str,
) -> (Simulation, RuleSet, u64, DriveCoord) {
    let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
        "[General]\nFlightLevel=1500\nPoseDir=0\n\
         [AircraftTypes]\n0=RETURN\n[BuildingTypes]\n0=PAD\n\
         [RETURN]\nStrength=150\nSpeed=14\nAmmo=1\nLandable=yes\n\
         AirportBound=yes\nFighter=yes\nSlowdownDistance=1000\nDock=PAD\n\
         Locomotor={{4A582746-9839-11d1-B709-00A024DDAFD1}}\n\
         ExtendedAircraftMissions.{key}={}\n\
         [PAD]\nStrength=500\nHelipad=yes\nNumberOfDocks={docks}\n{pad_extra}",
        if value { "yes" } else { "no" },
    )))
    .expect("rules");
    let mut sim = Simulation::new();
    let owner = sim.interner.intern("Americans");
    sim.houses
        .register(crate::sim::house_state::HouseState::new(
            owner, 0, None, true, 0, 10,
        ));
    let pad = sim
        .spawn_object("PAD", "Americans", 12, 10, 0, &rules)
        .expect("pad");
    let plane = sim
        .spawn_object("RETURN", "Americans", 10, 10, 0, &rules)
        .expect("aircraft");
    sim.substrate.set_aircraft_dock(plane, Some(pad));
    let entity = sim.substrate.entities.get_mut(plane).unwrap();
    entity.radio_contacts.insert(pad);
    entity
        .navigation
        .set_nav_com_for_test(Some(NavTargetRef::object(pad)));
    // The pad's coordinate: its own cell's centre, the destination the Fly
    // holds while it returns.
    let dock = DriveCoord {
        x: 12 * 256 + 128,
        y: 10 * 256 + 128,
        z: 0,
    };
    let fly = entity
        .locomotor
        .as_mut()
        .unwrap()
        .fly_runtime_mut()
        .unwrap();
    *fly = serde_json::from_value::<FlyRuntime>(serde_json::json!({
        "target_height": 1500,
        "taking_off": false,
        "landing": false,
        "destination": [dock.x, dock.y, dock.z],
        "cruise_mode": false,
        "moving": true,
        "airport_bound": true,
    }))
    .unwrap();
    (sim, rules, plane, dock)
}

/// Phobos `FlyLocomotionClass_FlightUpdate_SetPrimaryFacing`
/// (`Ext/Aircraft/Hooks.cpp:596-676`, hook `0x4CF190`): with `RearApproach`
/// off the heading aims at the dock; on, the destination shifts three cells
/// sideways (the landing face is south, the bearing east puts it on the west
/// side) so the heading curves in.
#[test]
fn rear_approach_offsets_the_landing_destination() {
    let heading = |value: bool| {
        let (mut sim, rules, plane, dock) = phobos_returning_aircraft("RearApproach", value, 1, "");
        let frame = sim.session.binary_frame();
        let owner = ground_pose::object_get_coords(
            sim.substrate.entities.get(plane).unwrap(),
            sim.resolved_terrain.as_ref(),
        );
        sim.fly_horizontal_step(
            plane,
            dock,
            true,
            &rules,
            None,
            crate::sim::world::FrameEffects::empty(),
        );
        let facing = sim
            .substrate
            .entities
            .get(plane)
            .unwrap()
            .body_facing
            .current(frame);
        (facing, owner)
    };
    let (stock, owner) = heading(false);
    assert_eq!(
        stock,
        crate::util::direction_tables::facing16_between(
            [owner.x, owner.y],
            [12 * 256 + 128, 10 * 256 + 128]
        ),
        "off keeps the stock heading at the dock"
    );
    let (rear, _) = heading(true);
    assert_eq!(
        rear,
        crate::util::direction_tables::facing16_between(
            [owner.x, owner.y],
            [12 * 256 + 128 - 3 * 256, 10 * 256 + 128 + 3 * 256]
        ),
        "on aims three cells west and three south of the dock"
    );
    assert_ne!(rear, stock);
}

/// Phobos `BuildingTypeExt::AircraftDockingDirs`
/// (`Ext/BuildingType/Body.cpp:287-305`) and `AircraftExt::GetLandingDir`
/// (`Ext/Aircraft/Body.cpp:146-161`): the aircraft's link index in the dock's
/// radio contacts selects the authored landing face, an absent index falls
/// back to slot 0, and a building without an authored slot keeps the stock
/// pad facing.
#[test]
fn aircraft_docking_dir_names_the_landing_face_per_link_index() {
    let pick_pad_link = |sim: &mut Simulation, plane: u64, slot: Option<usize>| {
        let pad = sim
            .substrate
            .entities
            .get(plane)
            .unwrap()
            .radio_contacts
            .slot(0)
            .expect("the plane holds its pad link");
        let pad_entity = sim.substrate.entities.get_mut(pad).unwrap();
        pad_entity.radio_contacts.set_capacity(3);
        let _ = pad_entity.radio_contacts.remove(plane);
        if let Some(slot) = slot {
            pad_entity.radio_contacts.set_slot(slot, plane);
        }
    };
    let (mut sim, rules, plane, _) = phobos_returning_aircraft(
        "RearApproach",
        false,
        3,
        "AircraftDockingDir=32\nAircraftDockingDir1=96\nAircraftDockingDir2=160\n",
    );
    let frame = sim.session.binary_frame();
    let facing = |sim: &Simulation| {
        crate::sim::aircraft::landing_base::get_landing_dir(
            sim.substrate.entities.get(plane).unwrap(),
            &sim.substrate.entities,
            frame,
            rules.general.pose_dir,
            Some((&rules, &sim.interner)),
        )
    };

    pick_pad_link(&mut sim, plane, Some(1));
    assert_eq!(facing(&sim), 96 << 8, "slot 1 names the landing face");
    pick_pad_link(&mut sim, plane, Some(2));
    assert_eq!(facing(&sim), 160 << 8, "slot 2 names its own face");
    pick_pad_link(&mut sim, plane, Some(0));
    assert_eq!(facing(&sim), 32 << 8, "slot 0 takes AircraftDockingDir");
    pick_pad_link(&mut sim, plane, None);
    assert_eq!(
        facing(&sim),
        32 << 8,
        "an aircraft without a link index falls back to slot 0's entry"
    );

    let (sim, rules, plane, _) = phobos_returning_aircraft("RearApproach", false, 1, "");
    let frame = sim.session.binary_frame();
    let stock = crate::sim::aircraft::landing_base::landing_direction(
        sim.substrate.entities.get(plane).unwrap(),
        &sim.substrate.entities,
        frame,
        rules.general.pose_dir,
    ) as u16;
    let via_phobos = crate::sim::aircraft::landing_base::get_landing_dir(
        sim.substrate.entities.get(plane).unwrap(),
        &sim.substrate.entities,
        frame,
        rules.general.pose_dir,
        Some((&rules, &sim.interner)),
    );
    assert_eq!(
        via_phobos,
        stock.wrapping_shl(13),
        "an unauthored slot keeps the stock pad facing"
    );
}

/// Phobos `FlyLocomotionClass_FlightUpdate_SetFlightLevel`
/// (`Ext/Aircraft/Hooks.cpp:669-671`, hook `0x4CF3D0`): under `EarlyDescend`
/// an aircraft within `SlowdownDistance` of the dock it returns to descends
/// proportionally instead of holding its flight level at 512 of 1000 leptons.
#[test]
fn early_descend_starts_the_slow_descent_within_slowdown_distance() {
    let level = |value: bool| {
        let (mut sim, rules, plane, dock) = phobos_returning_aircraft("EarlyDescend", value, 1, "");
        sim.fly_horizontal_step(
            plane,
            dock,
            true,
            &rules,
            None,
            crate::sim::world::FrameEffects::empty(),
        );
        let entity = sim.substrate.entities.get(plane).unwrap();
        entity
            .locomotor
            .as_ref()
            .unwrap()
            .fly_runtime()
            .unwrap()
            .target_height()
    };
    assert_eq!(level(false), 1500, "stock holds FlightLevel");
    // (1500 - (0 - 0 + 1)) * (512 / 1000) truncated, plus 1.
    assert_eq!(level(true), 768, "the returning aircraft descends early");
}
