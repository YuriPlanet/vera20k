use super::*;
use crate::rules::ini_parser::IniFile;
use crate::sim::game_entity::GameEntity;
use crate::sim::passenger::{PassengerCargo, PassengerRole};
use crate::sim::snapshot::GameSnapshot;
use crate::sim::vision::OwnerVisibility;
use crate::util::fixed_math::SimFixed;
use serde_json::{Value, json};

fn xyz(input: &Value, key: &str, default: [i32; 3]) -> [i32; 3] {
    input[key].as_array().map_or(default, |a| {
        std::array::from_fn(|n| a[n].as_i64().unwrap() as i32)
    })
}

fn insert(
    sim: &mut Simulation,
    id: u64,
    name: &str,
    category: EntityCategory,
    point: [i32; 3],
    marked: bool,
) {
    let owner = sim.interner.intern("AIRCRAFT_OWNER");
    let kind = sim.interner.intern(name);
    let mut entity = GameEntity::new_at_frame_zero_for_test(
        id,
        (point[0] / 256) as u16,
        (point[1] / 256) as u16,
        0,
        0,
        owner,
        crate::sim::components::Health { current: 100 },
        kind,
        category,
        0,
        5,
        true,
    );
    entity.position.sub_x = SimFixed::from_num(point[0] % 256);
    entity.position.sub_y = SimFixed::from_num(point[1] % 256);
    entity.position.exact_z_leptons = Some(point[2]);
    entity.lifecycle.object_alive = marked;
    entity.lifecycle.in_limbo = false;
    sim.substrate.entities.insert(entity);
    sim.substrate.next_stable_object_id = sim.substrate.next_stable_object_id.max(id + 1);
}

fn fixture(input: &Value) -> (Simulation, RuleSet) {
    let passengers = input["passengers"].as_array().cloned().unwrap_or_default();
    let mut definitions = String::new();
    let mut registered = String::new();
    for (n, passenger) in passengers.iter().enumerate() {
        registered.push_str(&format!("{}=PAX{n}\n", n + 3));
        definitions.push_str(&format!(
            "[PAX{n}]\nStrength=100\nWeaponCount=2\nTurretCount={}\n",
            passenger["turrets"].as_i64().unwrap_or(0)
        ));
        let mut weapons = String::new();
        for (key, prefix) in [("ranges", ""), ("elite_ranges", "Elite")] {
            let values = passenger[key].as_array().cloned().unwrap_or_else(|| {
                if key == "ranges" {
                    vec![json!(1024)]
                } else {
                    vec![]
                }
            });
            for (slot, value) in values.iter().enumerate() {
                if let Some(range) = value.as_i64() {
                    let name = format!("PAX{n}{prefix}{slot}");
                    let key = if passenger["turrets"].as_i64().unwrap_or(0) > 0 {
                        format!("{prefix}Weapon{}", slot + 1)
                    } else {
                        format!(
                            "{prefix}{}",
                            if slot == 0 { "Primary" } else { "Secondary" }
                        )
                    };
                    definitions.push_str(&format!("{key}={name}\n"));
                    weapons.push_str(&format!(
                        "[{name}]\nDamage=1\nRange={}\n",
                        range as f64 / 256.0
                    ));
                }
            }
        }
        definitions.push_str(&weapons);
    }
    let ini = format!(
        "[General]\nFlightLevel=1500\n[AircraftTypes]\n0=TEST\n[VehicleTypes]\n0=TARGET\n1=DEST\n2=BLOCKER\n{registered}\
         [BuildingTypes]\n0=BUILDING\n[BUILDING]\nFoundation=1x1\n\
         [TEST]\nStrength=100\nSpeed=8\nAmmo=2\nLandable=yes\nLocomotor={{4A582746-9839-11D1-B709-00A024DDAFD1}}\n\
         Primary={}\n{}AirportBound={}\nCarryall={}\nSpawned={}\nOpenTopped={}\n\
         [TARGET]\nStrength=100\n[DEST]\nStrength=100\n\
         [BLOCKER]\nStrength=100\nSpawned={}\nSpawns=TEST\nSpawnsNumber=1\n\
         [PRIMARY]\nDamage=1\nRange={}\nProjectile=PROJECTILE\n\
         [ELITE]\nDamage=1\nRange={}\nProjectile=PROJECTILE\n\
         [PROJECTILE]\nROT={}\nInviso={}\n{definitions}",
        if input["weapon"].as_bool().unwrap_or(true) {
            "PRIMARY"
        } else {
            "none"
        },
        if input["elite_weapon"].as_bool().unwrap_or(true) {
            "ElitePrimary=ELITE\n"
        } else {
            ""
        },
        input["airport_bound"].as_bool().unwrap_or(false),
        input["carryall"].as_bool().unwrap_or(false),
        input["spawned"].as_bool().unwrap_or(false),
        input["open_topped"].as_bool().unwrap_or(false),
        input["blocker_spawned"].as_bool().unwrap_or(false),
        input["range"].as_i64().unwrap_or(1536) as f64 / 256.0,
        input["elite_range"].as_i64().unwrap_or(2304) as f64 / 256.0,
        input["rot"].as_i64().unwrap_or(3),
        input["inviso"].as_bool().unwrap_or(false),
    );
    let rules = RuleSet::from_ini(&IniFile::from_str(&ini)).unwrap();
    let mut sim = Simulation::with_seed(input["seed"].as_u64().unwrap_or(31));
    sim.session.map_width = 128;
    sim.session.map_height = 128;
    sim.session.game_mode_nonzero = input["game_mode"].as_u64().unwrap_or(1) != 0;
    let local = input["local"]
        .as_array()
        .cloned()
        .unwrap_or_else(|| vec![json!(0), json!(0), json!(64), json!(64)]);
    sim.playfield_bounds = Some(
        crate::map::playfield::PlayfieldBounds::from_normalized_local_size(
            64,
            local[0].as_i64().unwrap() as i32,
            local[1].as_i64().unwrap() as i32,
            local[2].as_i64().unwrap() as i32,
            local[3].as_i64().unwrap() as i32,
        ),
    );
    super::super::lifecycle_tests::install_common_raw_terrain(&mut sim, 128, 128, 0, None);
    insert(
        &mut sim,
        1,
        "TEST",
        EntityCategory::Aircraft,
        xyz(input, "aircraft", [10368, 16512, 500]),
        input["include_self"].as_bool().unwrap_or(true),
    );
    // Native TARGET/DEST are not members of the supplied Foot vector. Their
    // physical centers remain readable; unmarked entries give the same search
    // scan here. include_self=false is likewise a membership contrast only,
    // not a claim that a live object's native registry has this history.
    let foot = input["target_flags"].as_i64().unwrap_or(5) & 4 != 0;
    insert(
        &mut sim,
        2,
        if foot { "TARGET" } else { "BUILDING" },
        if foot {
            EntityCategory::Unit
        } else {
            EntityCategory::Structure
        },
        xyz(input, "target", [16512, 16512, 0]),
        false,
    );
    insert(
        &mut sim,
        3,
        "DEST",
        EntityCategory::Unit,
        xyz(input, "destination", [20608, 16512, 0]),
        false,
    );
    if input["target_has_destination"].as_bool().unwrap_or(false) {
        sim.substrate
            .entities
            .get_mut(2)
            .unwrap()
            .navigation
            .nav_com = Some(NavTargetRef::Entity { id: 3 });
    }
    let owner = sim.substrate.entities.get_mut(1).unwrap();
    owner.veterancy = (input["veterancy"].as_u64().unwrap_or(0) * 100) as u16;
    if input["flag_3d4"].as_bool().unwrap_or(false) {
        owner.mark_mission_only();
    }
    if input["carryall_target"].as_bool().unwrap_or(false) {
        owner.navigation.nav_com = Some(NavTargetRef::Entity { id: 4 });
    }
    if input.get("blocker").is_some() {
        insert(
            &mut sim,
            4,
            "BLOCKER",
            EntityCategory::Unit,
            xyz(input, "blocker", [1, 1, 0]),
            true,
        );
        if input["blocker_spawn_manager"].as_bool().unwrap_or(false) {
            let manager = crate::sim::spawn_manager::init_spawn_manager(
                rules.object("BLOCKER").unwrap(),
                &rules,
                &mut sim.interner,
                100,
            );
            assert!(manager.is_some());
            sim.substrate.entities.get_mut(4).unwrap().spawn_manager = manager;
        }
        if input.get("blocker_cells").is_some() {
            sim.add_entity_occupancy(4);
        }
    }
    if let Some(reservations) = input["reserved"].as_array() {
        for (n, cell) in reservations.iter().enumerate() {
            let id = 10 + n as u64;
            insert(
                &mut sim,
                id,
                "TARGET",
                EntityCategory::Unit,
                [1, 1, 0],
                input["reservation_marked"].as_bool().unwrap_or(true),
            );
            let e = sim.substrate.entities.get_mut(id).unwrap();
            e.lifecycle.in_limbo = input["reservation_limbo"].as_bool().unwrap_or(false);
            e.navigation.nav_com = Some(NavTargetRef::cell(
                cell[0].as_u64().unwrap() as u16,
                cell[1].as_u64().unwrap() as u16,
            ));
        }
    }
    let mut cargo = PassengerCargo::new(100, 0);
    for (n, passenger) in passengers.iter().enumerate().rev() {
        let id = 500 + n as u64;
        insert(
            &mut sim,
            id,
            &format!("PAX{n}"),
            EntityCategory::Unit,
            [1, 1, 0],
            true,
        );
        let e = sim.substrate.entities.get_mut(id).unwrap();
        e.lifecycle.in_limbo = true;
        e.veterancy = (passenger["veterancy"].as_u64().unwrap_or(0) * 100) as u16;
        e.weapon_override = Some(combat_weapon::WeaponOverride::IfvSlot(
            passenger["current"].as_u64().unwrap_or(0) as u32,
        ));
        cargo.board_forced(id, 1);
    }
    sim.substrate.entities.get_mut(1).unwrap().passenger_role = PassengerRole::Transport { cargo };
    // The original global Cell flags belong to CurrentHouse, even when the
    // searching aircraft belongs to a different house. No UI cache is built.
    let viewer = sim.interner.intern("CURRENT_HOUSE");
    sim.session.current_house = Some(viewer);
    sim.houses.insert(
        viewer,
        crate::sim::house_state::HouseState::new(viewer, 0, None, true, 0, 1),
    );
    sim.session.house_order.push(viewer);
    let mut view = serde_json::to_value(OwnerVisibility::new(128, 128)).unwrap();
    for cell in view["cell_runtime"].as_array_mut().unwrap() {
        cell["alt_flags"] = json!(if input["visible"].as_bool().unwrap_or(true) {
            16
        } else {
            0
        });
    }
    for (key, flag) in [
        ("hidden", 0),
        ("revealed", input["reveal_bits"].as_u64().unwrap_or(16)),
    ] {
        if let Some(cells) = input[key].as_array() {
            for cell in cells {
                let index =
                    cell[1].as_u64().unwrap() as usize * 128 + cell[0].as_u64().unwrap() as usize;
                view["cell_runtime"][index]["alt_flags"] = json!(flag);
            }
        }
    }
    sim.fog
        .by_owner
        .insert(viewer, serde_json::from_value(view).unwrap());
    (sim, rules)
}

fn reengagement_fixture(input: &Value) -> (Simulation, RuleSet) {
    use crate::sim::aircraft::AircraftMission;
    use crate::sim::docking::aircraft_dock::AircraftAmmo;
    use crate::sim::mission::{MissionDispatchTimer, MissionId, MissionLeafState};
    use crate::sim::movement::locomotor::LocomotorState;
    let mut case = input.clone();
    case["open_topped"] = json!(input["open_transport"].as_bool().unwrap_or(false));
    let (mut sim, mut rules) = fixture(&case);
    rules.general.blockage_path_delay_ticks = 11;
    rules.mission_control =
        crate::rules::mission_data::MissionControl::from_ini(&IniFile::from_str(&format!(
            "[Attack]\nRate={}\n",
            input["rate"].as_f64().unwrap_or(0.016)
        )));
    sim.session.binary_frame = 100;
    if input["open_transport"].as_bool().unwrap_or(false) {
        insert(
            &mut sim,
            4,
            "TEST",
            EntityCategory::Aircraft,
            [1, 1, 0],
            false,
        );
    }
    // Native target's virtual +4C resolves through its supplied Fly receiver.
    sim.substrate.entities.get_mut(2).unwrap().locomotor = Some(LocomotorState::from_object_type(
        rules.object("TEST").unwrap(),
        0,
    ));
    let entity = sim.substrate.entities.get_mut(1).unwrap();
    entity
        .mission
        .apply_test_fixture(crate::sim::mission::state::MissionTestFixture {
            current: MissionId::from_raw(1),
            suspended: MissionId::NONE,
            queued: MissionId::NONE,
            movement_bypass_latch: 0,
            handler_state: 1,
            mission_start_frame: 0,
            ai_counter: 0,
            dispatch_timer: MissionDispatchTimer::at_frame(100),
        });
    entity.mission_leaf = MissionLeafState::aircraft_raw_for_test(1, 1, false);
    entity.aircraft_mission = Some(AircraftMission::Attack { sub_state: 1 });
    entity.attack_target = (!input["null_target"].as_bool().unwrap_or(false))
        .then(|| crate::sim::combat::AttackTarget::new(2));
    let mut ammo = AircraftAmmo::new(2);
    ammo.current = input["ammo"].as_i64().unwrap_or(2) as i32;
    if input["pending"].as_bool().unwrap_or(false) {
        ammo.begin_release();
    }
    entity.aircraft_ammo = Some(ammo);
    entity.navigation.nav_com_aux = Some(NavTargetRef::Entity { id: 2 });
    entity.navigation.nav_com = input["old_nav"]
        .as_bool()
        .unwrap_or(false)
        .then(|| NavTargetRef::cell(32, 32));
    entity.foot_locomotor_swap_active = input["swap"].as_bool().unwrap_or(false);
    if input["open_transport"].as_bool().unwrap_or(false) {
        entity.passenger_role = PassengerRole::Inside { transport_id: 4 };
    }
    if input["bunker"].as_bool().unwrap_or(false) {
        entity.bunker_link = crate::sim::game_entity::BunkerLink::Installed(2);
    }
    let path = &mut entity.navigation.path_runtime;
    path.movement_timer = crate::sim::timer::CdTimer::from_raw(7, 23);
    path.blocked_timer = crate::sim::timer::CdTimer::from_raw(9, 29);
    path.retries_left = 19;
    path.path_blocked = true;
    let mut loco = LocomotorState::from_object_type(rules.object("TEST").unwrap(), 0);
    loco.powered = input["powered"].as_bool().unwrap_or(true);
    loco.set_fly_target_height(37);
    loco.fly_runtime_mut().unwrap().retain_destination(
        DriveCoord {
            x: 8192,
            y: 8192,
            z: 1500,
        },
        None,
        || unreachable!(),
    );
    entity.locomotor = Some(loco);
    sim.set_logic_order_for_test(vec![1]);
    (sim, rules)
}

fn assert_reengagement(sim: &mut Simulation, row: &Value) {
    let entity = sim.substrate.entities.get(1).unwrap();
    let nav = match entity.navigation.nav_com {
        Some(NavTargetRef::Cell { rx, ry }) => json!([rx, ry]),
        Some(NavTargetRef::Entity { id: 2 }) => json!("target"),
        None => Value::Null,
        other => panic!("unexpected NavCom: {other:?}"),
    };
    let crate::sim::aircraft::AircraftMission::Attack { sub_state } =
        entity.aircraft_mission.as_ref().unwrap()
    else {
        panic!("Attack must remain active");
    };
    let fly = entity.locomotor.as_ref().unwrap().fly_runtime().unwrap();
    let point = fly.destination();
    let path = &entity.navigation.path_runtime;
    let fields = json!({
        "state": sub_state,
        "delay": entity.mission.dispatch_timer().delay(),
        "nav": nav, "aux": entity.navigation.nav_com_aux.is_some(),
        "ammo": entity.aircraft_ammo.as_ref().unwrap().current,
        "pending": entity.aircraft_ammo.as_ref().unwrap().release_pending(),
        "latch": entity.mission_leaf.as_aircraft().unwrap().action_latch() != 0,
        "destination": [point.x, point.y, point.z], "moving": fly.moving(),
        "height": fly.target_height(),
        "movement_timer": [path.movement_timer.start_frame(), path.movement_timer.duration()],
        "blocked_timer": [path.blocked_timer.start_frame(), path.blocked_timer.duration()],
        "retry": path.retries_left, "blocked": path.path_blocked,
    });
    for (key, value) in fields.as_object().unwrap() {
        assert_eq!(value, &row[key], "{} {key}", row["input"]);
    }
    assert_eq!(entity.mission.handler_state(), u32::from(*sub_state));
    assert_eq!(entity.mission.dispatch_timer().start_frame(), 100);
    assert_eq!(
        sim.scenario_rng.next_u32(),
        row["next_random"].as_u64().unwrap() as u32
    );
}

#[test]
fn aircraft_reengagement_matches_original_through_production_dispatch() {
    let rows: Vec<Value> = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/aircraft_reengagement.json"
    ))
    .unwrap();
    assert_eq!(rows.len(), 16);
    for row in rows {
        let (mut sim, rules) = reengagement_fixture(&row["input"]);
        let saved = GameSnapshot::save(&sim, 0, 0, "aircraft re-engagement", 0);
        let mut restored = GameSnapshot::load(&saved).unwrap().sim;
        restored.restore_after_snapshot_load().unwrap();
        // The snapshot envelope's seed-reset policy is independent of the
        // persisted mission/navigation inputs compared in this fixture.
        restored.scenario_rng = sim.scenario_rng.clone();
        assert_eq!(sim.state_hash(), restored.state_hash());
        for world in [&mut sim, &mut restored] {
            crate::sim::aircraft::tick_aircraft_missions(world, &rules, None);
            let hash = world.state_hash();
            // A second same-frame call must respect the native returned delay.
            crate::sim::aircraft::tick_aircraft_missions(world, &rules, None);
            assert_eq!(world.state_hash(), hash);
            assert_reengagement(world, &row);
        }
        assert_eq!(sim.state_hash(), restored.state_hash());
    }
}

#[test]
fn aircraft_reengagement_reserves_in_logic_order_and_ignores_inactive_objects() {
    let rows: Vec<Value> = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/aircraft_reengagement.json"
    ))
    .unwrap();
    let base = rows.iter().find(|r| r["input"]["name"] == "base").unwrap();
    let run = |order: Vec<u64>| {
        let (mut sim, rules) = reengagement_fixture(&base["input"]);
        for id in [7, 9] {
            let mut other = sim.substrate.entities.get(1).unwrap().clone();
            other.stable_id = id;
            sim.substrate.entities.insert(other);
        }
        sim.substrate.next_stable_object_id = 10;
        sim.set_logic_order_for_test(order.clone());
        crate::sim::aircraft::tick_aircraft_missions(&mut sim, &rules, None);
        let first = sim.substrate.entities.get(order[0]).unwrap();
        assert_eq!(selected(first.navigation.nav_com), base["nav"]);
        assert_eq!(
            first.mission.dispatch_timer().delay(),
            base["delay"].as_i64().unwrap() as i32
        );
        let second = sim.substrate.entities.get(order[1]).unwrap();
        assert!(second.navigation.nav_com.is_some());
        assert_ne!(
            first.navigation.nav_com, second.navigation.nav_com,
            "later search observes the first live reservation"
        );
        let inactive = sim.substrate.entities.get(9).unwrap();
        assert_eq!(inactive.mission.handler_state(), 1);
        assert_eq!(inactive.mission.dispatch_timer().delay(), 0);
        assert_eq!(inactive.navigation.nav_com, None);
        (
            first.navigation.nav_com,
            second.navigation.nav_com,
            second.mission.dispatch_timer().delay(),
            sim.scenario_rng.logical_state(),
        )
    };
    // Stable-ID order is identical; reversing only Logic order changes which
    // aircraft receives the native first result and its Scenario RNG draws.
    assert_eq!(run(vec![7, 1]), run(vec![1, 7]));
}

fn selected(value: Option<NavTargetRef>) -> Value {
    match value {
        None => Value::Null,
        Some(NavTargetRef::Cell { rx, ry }) => json!([rx as i16, ry as i16]),
        Some(NavTargetRef::Entity { id: 2 }) => json!("target"),
        other => panic!("unexpected returned identity {other:?}"),
    }
}

#[test]
fn fire_location_live_inputs_and_rng_match_original_search() {
    let rows: Vec<Value> = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/aircraft_fire_location.json"
    ))
    .unwrap();
    assert_eq!(rows.len(), 63);
    for row in rows {
        let (mut sim, rules) = fixture(&row["input"]);
        let e = sim.substrate.entities.get(1).unwrap();
        assert_eq!(
            combat_weapon::weapon_range(
                e,
                rules.object("TEST").unwrap(),
                0,
                &sim.substrate.entities,
                &rules,
                &sim.interner
            ) as i64,
            row["weapon_range"].as_i64().unwrap(),
            "{} range",
            row["input"]
        );
        let target = (!row["input"]["null_target"].as_bool().unwrap_or(false))
            .then_some(NavTargetRef::Entity { id: 2 });
        let before = sim.scenario_rng.logical_state();
        let result = sim.aircraft_find_fire_location(1, target, &rules);
        assert_eq!(selected(result), row["result"], "{}", row["input"]);
        assert_eq!(
            before != sim.scenario_rng.logical_state(),
            row["rng_changed"].as_bool().unwrap(),
            "{}",
            row["input"]
        );
        assert_eq!(
            sim.scenario_rng.next_u32() as u64,
            row["next_random"].as_u64().unwrap(),
            "{}",
            row["input"]
        );
    }
}

#[test]
fn fire_location_reads_restored_reservations_cargo_and_current_house() {
    let rows: Vec<Value> = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/aircraft_fire_location.json"
    ))
    .unwrap();
    for name in [
        "reserved_live",
        "target_destination",
        "cargo_turret_slot",
        "cargo_elite",
        "only_inner_ring",
        "carryall_skips_its_unit",
    ] {
        let row = rows.iter().find(|r| r["input"]["name"] == name).unwrap();
        let (mut sim, rules) = fixture(&row["input"]);
        let bytes = GameSnapshot::save(&sim, 0, 0, "aircraft fire location", 0);
        let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
        restored.restore_after_snapshot_load().unwrap();
        // Save loading intentionally reinitializes Scenario RNG. Supply the
        // same continuation stream to isolate persistence of search inputs.
        restored.scenario_rng = sim.scenario_rng.clone();
        for world in [&mut sim, &mut restored] {
            assert_eq!(
                selected(world.aircraft_find_fire_location(
                    1,
                    Some(NavTargetRef::Entity { id: 2 }),
                    &rules
                )),
                row["result"],
                "{name}"
            );
            assert_eq!(
                world.scenario_rng.next_u32() as u64,
                row["next_random"].as_u64().unwrap(),
                "{name}"
            );
        }
        assert_eq!(sim.state_hash(), restored.state_hash(), "{name}");
    }
}
