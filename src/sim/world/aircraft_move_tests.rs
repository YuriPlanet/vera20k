//! Replays tools/spatial_oracle/aircraft_move.json: the original
//! Find_Attack_Cell 0x00418E20 and Mission_Move 0x004166E0 on supplied map
//! states, through the production search and the world queries the
//! production Mission_Move host gives `move_mission::move_visit`. The oracle
//! stubs the locomotor and the destination and idle virtuals; so does
//! [`RowHost`], recording their calls.
use super::*;
use crate::map::entities::EntityCategory;
use crate::rules::ini_parser::IniFile;
use crate::sim::game_entity::GameEntity;
use crate::sim::mission::{MissionDispatchTimer, MissionId};
use crate::util::fixed_math::SimFixed;
use serde_json::{Value, json};

const OWNER: u64 = 1;
const BLOCKER: u64 = 4;

///705D60 is called by Fly Begin_Landing4CFA87 and the Jumpjet arrival
///transitions54BE2F/54C28C. Exercise those hosts through real frame work.
#[test]
fn fly_and_jumpjet_planned_move_legs_progress_through_native_arrival() {
    use crate::sim::command::{Command, CommandEnvelope};
    for name in ["HORNET", "JUMPJET"] {
        let (mut sim, rules, registry) =
            crate::sim::world::entry_test_fixture::fixture_with_rules("");
        let owner = sim.interner.intern("Americans");
        sim.houses.insert(
            owner,
            crate::sim::house_state::HouseState::new(owner, 0, None, true, 0, 10),
        );
        sim.session.house_order = vec![owner];
        sim.session.current_house = Some(owner);
        // Ordinary skirmish paths: Aircraft4196B0's campaign shroud gate
        // must not reject this fixture's initially unexplored spawn cell.
        sim.session.set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true),
        );
        // This route fixture has no base; keep its house alive while its
        // aircraft execute the path instead of triggering ShortGame defeat.
        sim.session.game_options.short_game = false;
        let actor = sim
            .spawn_object_with_overlay_registry(name, "Americans", 15, 15, 0, &rules, &registry)
            .unwrap();
        for target_rx in [18, 21] {
            let record = sim
                .encode_planning_order_record(
                    owner,
                    &Command::Move {
                        entity_id: actor,
                        target_rx,
                        target_ry: 15,
                    },
                )
                .unwrap();
            let envelope = sim
                .decode_native_command_record(&record, sim.session.tick)
                .unwrap();
            assert!(sim.apply_command("Americans", &envelope.payload, Some(&rules)));
        }
        let record = sim.encode_plan_commit_record(owner).unwrap();
        let commit = CommandEnvelope::new(
            owner,
            sim.session.tick + 1,
            Command::PlanningRecord { record },
        );
        let grid = sim.path_grid.clone();
        sim.advance_tick(
            &[commit],
            Some(&rules),
            grid.as_deref(),
            Some(&registry),
            67,
        );
        let mut saw_first = false;
        let mut finished = false;
        for _ in 0..1200 {
            let due = sim.take_due_commands();
            sim.advance_tick(&due, Some(&rules), grid.as_deref(), Some(&registry), 67);
            let entity = sim.substrate.entities.get(actor).unwrap();
            saw_first |=
                entity.position.rx().abs_diff(18) <= 1 && entity.position.ry().abs_diff(15) <= 1;
            if entity.position.rx().abs_diff(21) <= 1
                && entity.position.ry().abs_diff(15) <= 1
                && sim.planning_paths(owner).is_empty()
            {
                finished = true;
                break;
            }
        }
        assert!(
            saw_first,
            "{name}: production movement visits the first leg; position={:?}, plans={:?}, queued={:?}",
            sim.substrate
                .entities
                .get(actor)
                .map(|entity| entity.position),
            sim.planning_paths(owner),
            sim.pending_command_snapshot(),
        );
        assert!(
            finished,
            "{name}: native arrival starts and completes the second leg"
        );
    }
}

fn vectors() -> &'static Value {
    crate::test_fixture::json("tools/spatial_oracle/aircraft_move.json")
}

fn cell_of(value: &Value) -> (u16, u16) {
    (
        value[0].as_u64().unwrap() as u16,
        value[1].as_u64().unwrap() as u16,
    )
}

fn center(cell: (u16, u16)) -> [i32; 3] {
    [
        i32::from(cell.0) * 256 + 128,
        i32::from(cell.1) * 256 + 128,
        0,
    ]
}

fn insert(sim: &mut Simulation, id: u64, name: &str, category: EntityCategory, point: [i32; 3]) {
    let owner = sim.interner.intern("AIRCRAFT_OWNER");
    let kind = sim.interner.intern(name);
    let mut entity = GameEntity::new_at_frame_zero_for_test(
        id,
        (point[0] / 256) as u16,
        (point[1] / 256) as u16,
        0,
        0,
        owner,
        crate::sim::combat::Health::new(100),
        kind,
        category,
        0,
        5,
        true,
    );
    entity
        .position
        .set_sub_x_for_test(SimFixed::from_num(point[0] % 256));
    entity
        .position
        .set_sub_y_for_test(SimFixed::from_num(point[1] % 256));
    entity.position.set_exact_z_leptons_for_test(Some(point[2]));
    entity.lifecycle.set_object_alive_for_test(true);
    entity.lifecycle.set_in_limbo_for_test(false);
    sim.substrate.entities.insert(entity);
    sim.substrate.next_stable_object_id = sim.substrate.next_stable_object_id.max(id + 1);
}

/// The oracle's map: flat Clear 128x128 cells, MapSize width 64 and the raw
/// LocalSize, the owner aircraft (no team, no planning path), a Unit
/// blocker in the Foot vector, and reserving aircraft parked off the
/// playfield.
fn fixture(input: &Value) -> (Simulation, RuleSet) {
    let ini = format!(
        "[AircraftTypes]\n0=TEST\n[VehicleTypes]\n0=BLOCKER\n\
         [TEST]\nStrength=100\nSpeed=8\nLandable=yes\nSpawned={}\n\
         Locomotor={{4A582746-9839-11D1-B709-00A024DDAFD1}}\n\
         [BLOCKER]\nStrength=100\nSpawned={}\nSpawns=TEST\nSpawnsNumber=1\n\
         Locomotor={{4A582744-9839-11D1-B709-00A024DDAFD1}}\n\
         [Clear]\nTrack=100%\n",
        input["spawned"].as_bool().unwrap_or(false),
        input["blocker_spawned"].as_bool().unwrap_or(false),
    );
    let mut rules = RuleSet::from_ini(&IniFile::from_str(&ini)).unwrap();
    rules.mission_control =
        crate::rules::mission_data::MissionControl::from_ini(&IniFile::from_str(&format!(
            "[Move]\nRate={}\n",
            input["rate"].as_f64().unwrap_or(0.016)
        )));
    let mut sim = Simulation::with_seed(input["seed"].as_u64().unwrap_or(31));
    sim.session.map_width = 128;
    sim.session.map_height = 128;
    let local = input["local"]
        .as_array()
        .cloned()
        .unwrap_or_else(|| vec![json!(0), json!(0), json!(64), json!(64)]);
    let local: Vec<i32> = local.iter().map(|v| v.as_i64().unwrap() as i32).collect();
    sim.playfield_bounds = Some(
        crate::map::playfield::PlayfieldBounds::from_normalized_local_size(
            64, local[0], local[1], local[2], local[3],
        ),
    );
    super::super::lifecycle_tests::install_common_raw_terrain(&mut sim, 128, 128, 0, None);

    let owner_cell = input.get("owner_cell").map_or((60, 64), cell_of);
    insert(
        &mut sim,
        OWNER,
        "TEST",
        EntityCategory::Aircraft,
        center(owner_cell),
    );
    let blocker_at = input.get("blocker_at").map_or((1, 1), cell_of);
    insert(
        &mut sim,
        BLOCKER,
        "BLOCKER",
        EntityCategory::Unit,
        center(blocker_at),
    );
    // The blocker's +0x4C reads its locomotor's Destination.
    let blocker = sim.substrate.entities.get_mut(BLOCKER).unwrap();
    let mut locomotor = crate::sim::movement::locomotor::LocomotorState::from_object_type(
        rules.object("BLOCKER").unwrap(),
        0,
    );
    locomotor.set_step_head(input.get("blocker_destination").map(|d| {
        crate::sim::components::DriveCoord {
            x: d[0].as_i64().unwrap() as i32,
            y: d[1].as_i64().unwrap() as i32,
            z: d[2].as_i64().unwrap() as i32,
        }
    }));
    blocker.locomotor = Some(locomotor);
    if input["blocker_spawn_manager"].as_bool().unwrap_or(false) {
        let manager = crate::sim::spawn_manager::init_spawn_manager(
            rules.object("BLOCKER").unwrap(),
            &rules,
            &mut sim.interner,
            100,
        );
        assert!(manager.is_some());
        sim.substrate
            .entities
            .get_mut(BLOCKER)
            .unwrap()
            .spawn_manager = manager;
    }
    // Native lists the one blocker in each supplied cell; a cell other than
    // its own gets a Unit of the same type, which every IsLandZoneClear
    // refuses alike before Is_Cell_Free_For_Landing could tell them apart.
    for (n, cell) in input["blocker_cells"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
    {
        let cell = cell_of(cell);
        let id = if cell == blocker_at {
            BLOCKER
        } else {
            let id = 20 + n as u64;
            insert(&mut sim, id, "BLOCKER", EntityCategory::Unit, center(cell));
            id
        };
        sim.add_entity_occupancy(id);
    }
    for (n, cell) in input["reserved"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
    {
        let id = 10 + n as u64;
        insert(
            &mut sim,
            id,
            "TEST",
            EntityCategory::Aircraft,
            [128, 128, 0],
        );
        let (rx, ry) = cell_of(cell);
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .navigation
            .set_nav_com_for_test(Some(NavTargetRef::cell(rx, ry)));
    }
    for cell in input["occupied"].as_array().into_iter().flatten() {
        let (x, y) = cell_of(cell);
        sim.substrate.raw_cell_occupation.mark_ground(x, y, 0x20);
    }
    (sim, rules)
}

/// The raw Scenario words the traced production draws consumed, as the
/// oracle records each RandomRanged rejection-sampling word (`0x0065C87C`).
fn raw_words(draws: &[Value]) -> Value {
    draws.iter().map(|draw| draw["value"].clone()).collect()
}

fn param(input: &Value) -> Option<NavTargetRef> {
    match &input["param"] {
        Value::Null => None,
        Value::String(name) if name == "blocker" => Some(NavTargetRef::Object { id: BLOCKER }),
        cell => {
            let (rx, ry) = cell_of(cell);
            Some(NavTargetRef::cell(rx, ry))
        }
    }
}

fn name(target: Option<NavTargetRef>) -> Value {
    match target {
        None => Value::Null,
        Some(NavTargetRef::Cell { rx, ry }) => json!(["cell", rx, ry]),
        Some(NavTargetRef::Object { id: BLOCKER }) => json!("blocker"),
        Some(other) => panic!("unexpected destination {other:?}"),
    }
}

#[test]
fn find_attack_cell_matches_original_searches() {
    let vectors = vectors();
    let rows = vectors["find_attack_cell"].as_array().unwrap();
    assert_eq!(rows.len(), 22);
    for row in rows {
        let input = &row["input"];
        let label = input["name"].as_str().unwrap();
        let (mut sim, rules) = fixture(input);
        let (result, draws) = crate::sim::rng::trace_draws(|| {
            sim.aircraft_find_attack_cell(OWNER, param(input), &rules)
        });
        assert_eq!(name(result), row["result"], "{label}");
        assert_eq!(raw_words(&draws), row["raw"], "{label}");
        assert_eq!(
            u64::from(sim.scenario_rng.next_u32()),
            row["next_random"].as_u64().unwrap(),
            "{label}"
        );
    }
}

/// The oracle's Mission_Move world: stubbed locomotor and destination/idle
/// virtuals, the production queries for the rest.
struct RowHost<'a> {
    sim: &'a mut Simulation,
    rules: &'a RuleSet,
    moving: bool,
    calls: Vec<Value>,
}

impl move_mission::MoveHost for RowHost<'_> {
    fn nav_com(&self) -> bool {
        self.sim
            .substrate
            .entities
            .get(OWNER)
            .unwrap()
            .navigation
            .nav_com()
            .is_some()
    }

    fn enter_idle_mode(&mut self) {
        self.calls.push(json!(["enter_idle_mode", 0, 1]));
    }

    fn assign_attack_cell(&mut self) {
        let nav = self
            .sim
            .substrate
            .entities
            .get(OWNER)
            .unwrap()
            .navigation
            .nav_com();
        let cell = self.sim.aircraft_find_attack_cell(OWNER, nav, self.rules);
        self.calls
            .push(json!(["assign_destination", name(cell), 1]));
    }

    fn move_to_nav_com(&mut self) {
        let nav = self
            .sim
            .substrate
            .entities
            .get(OWNER)
            .unwrap()
            .navigation
            .nav_com();
        let coord = nav_target_coordinate(
            nav.unwrap(),
            Some(OWNER),
            self.sim,
            self.sim.resolved_terrain.as_ref(),
            Some((self.rules, &self.sim.interner)),
        )
        .unwrap();
        self.calls
            .push(json!(["move_to", [coord.x, coord.y, coord.z]]));
    }

    fn is_moving(&mut self) -> bool {
        self.calls.push(json!(["is_moving"]));
        self.moving
    }

    fn nav_cell_is_own(&mut self) -> bool {
        self.sim.aircraft_nav_cell_is_own(OWNER, self.rules)
    }

    fn nav_cell_free(&mut self) -> bool {
        self.sim.aircraft_nav_cell_free(OWNER, self.rules)
    }

    fn epilogue(&mut self) -> i32 {
        self.sim
            .mission_rate_epilogue_for(self.rules, OWNER, MissionType::Move)
    }
}

#[test]
fn mission_move_matches_original_states() {
    let vectors = vectors();
    let rows = vectors["mission_move"].as_array().unwrap();
    assert_eq!(rows.len(), 47);
    for row in rows {
        let input = &row["input"];
        let label = input["name"].as_str().unwrap();
        let (mut sim, rules) = fixture(input);
        let owner = sim.substrate.entities.get_mut(OWNER).unwrap();
        owner.navigation.set_nav_com_for_test(param(input));
        owner
            .mission
            .apply_test_fixture(crate::sim::mission::state::MissionTestFixture {
                current: MissionId::from_known(MissionType::Move),
                suspended: MissionId::NONE,
                queued: MissionId::NONE,
                movement_bypass_latch: 0,
                handler_state: 0,
                mission_start_frame: 0,
                ai_counter: 0,
                dispatch_timer: MissionDispatchTimer::at_frame(0),
            });
        let mut host = RowHost {
            sim: &mut sim,
            rules: &rules,
            moving: input["moving"].as_bool().unwrap(),
            calls: Vec::new(),
        };
        let state = input["state"].as_u64().unwrap() as u8;
        let ((state, delay), draws) =
            crate::sim::rng::trace_draws(|| move_mission::move_visit(state, &mut host));
        let calls = std::mem::take(&mut host.calls);
        assert_eq!(i64::from(state), row["state"].as_i64().unwrap(), "{label}");
        assert_eq!(i64::from(delay), row["delay"].as_i64().unwrap(), "{label}");
        assert_eq!(Value::from(calls), row["calls"], "{label}");
        assert_eq!(raw_words(&draws), row["raw"], "{label}");
        assert_eq!(
            u64::from(sim.scenario_rng.next_u32()),
            row["next_random"].as_u64().unwrap(),
            "{label}"
        );
    }
}

/// Production composition witness on retail Hills: a Harrier ordered onto a
/// tank's cell takes the Move order's NavCom (Aircraft41AA80), and its first
/// Mission_Move visit's Find_Attack_Cell moves the NavCom to a free cell
/// beside the tank. AirportBound, every cell is free for it
/// (`0x00419B98`), so Mission_Move runs 2 into 4 and, over that cell, waits
/// in state 3 until its Fly stops. State 3 then enters idle mode
/// (`0x004176F0`), which drops the NavCom and asks for a dock (vt+0x528).
/// Its owner has no airfield, so the AirportBound Harrier crashes there
/// (vt+0x3DC). The native comparisons are the Mission_Move replay above and
/// the idle-mode replay (`aircraft::idle_entry`); this run claims no
/// whole-flight comparison.
#[test]
#[ignore = "requires the configured retail install and stock Hills.mmx"]
fn retail_harrier_sent_onto_a_tank_stops_beside_it_and_crashes_without_an_airfield() {
    use crate::headless_scenario::SIM_TICK_MS;
    use crate::sim::command::{Command, CommandEnvelope};
    let retail = std::env::var("RA2_DIR")
        .map(std::path::PathBuf::from)
        .expect("RA2_DIR names the retail install");
    let mut scenario = crate::headless_scenario::load(&retail, "Hills.mmx", 0x0B21_D6E5)
        .expect("load retail Hills through the production loader");
    let candidates = super::super::rocket_flight::retail_tests::sites(&scenario);
    let owner = scenario.sim().session.current_house.expect("order house");
    let owner_name = scenario.sim().interner.resolve(owner).to_owned();
    let runtime = &mut scenario.runtime;
    let rules = &runtime.resources.rules;
    let sim = &mut runtime.simulation;
    let ((start, target_cell), harrier, tank) = candidates
        .into_iter()
        .find_map(|(start, target)| {
            let tank = sim.spawn_object("MTNK", &owner_name, target.0, target.1, 0, rules)?;
            match sim.spawn_object("ORCA", &owner_name, start.0, start.1, 0, rules) {
                Some(harrier) => Some(((start, target), harrier, tank)),
                None => {
                    sim.uninit(tank);
                    None
                }
            }
        })
        .expect("Hills admits a Harrier and a tank on open level ground");
    sim.resolve_type_handles(rules);
    let mut navs = Vec::new();
    let mut moves = Vec::new();
    let mut crashed = None;
    for frame in 0..600 {
        let commands = if frame == 0 {
            vec![CommandEnvelope::new(
                owner,
                runtime.simulation.session.tick + 1,
                Command::Move {
                    entity_id: harrier,
                    target_rx: target_cell.0,
                    target_ry: target_cell.1,
                },
            )]
        } else {
            Vec::new()
        };
        runtime
            .advance_frame_for_tooling(&commands, SIM_TICK_MS)
            .expect("advance production frame");
        let Some(entity) = runtime.simulation.substrate.entities.get(harrier) else {
            break;
        };
        let nav = entity.navigation.nav_com();
        if navs.last().is_none_or(|&(_, last)| last != nav) {
            navs.push((frame, nav));
        }
        if entity.health.current() == 0 {
            crashed.get_or_insert((frame, (entity.position.rx(), entity.position.ry())));
            continue;
        }
        // The order queues Move; Ready_To_Commence starts it.
        if entity.mission.current() != MissionId::from_known(MissionType::Move) {
            assert!(moves.is_empty(), "the Harrier stays on Move: {moves:?}");
            continue;
        }
        let sub_state = crate::sim::aircraft::handler_state(entity);
        if moves.last().is_none_or(|&(_, last, _)| last != sub_state) {
            moves.push((
                frame,
                sub_state,
                (entity.position.rx(), entity.position.ry()),
            ));
        }
    }
    println!(
        "Harrier from {start:?} onto MTNK {tank} at {target_cell:?}: NavComs {navs:?}, \
         Mission_Move {moves:?}, crashed {crashed:?}"
    );
    assert_eq!(
        navs[0].1,
        Some(NavTargetRef::cell(target_cell.0, target_cell.1))
    );
    let Some(NavTargetRef::Cell { rx, ry }) = navs[1].1 else {
        panic!("Find_Attack_Cell picks a cell: {navs:?}");
    };
    let beside = (rx, ry);
    assert!(
        beside != target_cell && rx.abs_diff(target_cell.0) <= 2 && ry.abs_diff(target_cell.1) <= 2,
        "a free cell beside the tank: {navs:?}"
    );
    assert_eq!(navs.len(), 3, "the NavCom stays on that cell: {navs:?}");
    assert_eq!(navs[2].1, None, "until idle mode drops it");
    let states: Vec<u8> = moves.iter().map(|&(_, state, _)| state).collect();
    assert_eq!(states, [0, 1, 2, 4, 3], "{moves:?}");
    assert_eq!(moves[4].2, beside, "state 3 begins over that cell");
    let (crash_frame, crash_cell) = crashed.expect("the Harrier crashes once its Fly stops");
    assert_eq!(crash_frame, navs[2].0, "as idle mode drops the NavCom");
    assert_eq!(crash_cell, beside, "over that cell");
    assert!(
        runtime.simulation.substrate.entities.get(harrier).is_none(),
        "and is gone after its fall"
    );
}

fn set_current(sim: &mut Simulation, mission: MissionType) {
    sim.substrate
        .entities
        .get_mut(OWNER)
        .unwrap()
        .mission
        .apply_test_fixture(crate::sim::mission::state::MissionTestFixture {
            current: MissionId::from_known(mission),
            suspended: MissionId::NONE,
            queued: MissionId::NONE,
            movement_bypass_latch: 0,
            handler_state: 0,
            mission_start_frame: 0,
            ai_counter: 0,
            dispatch_timer: MissionDispatchTimer::at_frame(0),
        });
}

fn queue(sim: &mut Simulation, mission: MissionType) {
    sim.mission_queue_exact(
        OWNER,
        MissionId::from_known(mission),
        0,
        0,
        &crate::sim::mission::authority::EntityReadyInputProvider,
    )
    .unwrap();
}

/// A Move order or a spawn manager's Move reaching an aircraft whose
/// Mission_Attack holds the release latch (`+0x6D2`): `ReadyToCommence`
/// refuses while it is up, so Mission_Attack keeps running from its own
/// state until one of its entries clears it, and the Commence that follows
/// starts Mission_Move at state 0 (Commence zeroes Mission+0xBC).
#[test]
fn a_move_queued_during_a_release_waits_for_mission_attack() {
    let (mut sim, rules) = fixture(&json!({}));
    set_current(&mut sim, MissionType::Attack);
    let plane = sim.substrate.entities.get_mut(OWNER).unwrap();
    plane
        .mission
        .set_current_for_test(MissionId::from_known(MissionType::Attack), 5);
    plane.mission_leaf.set_aircraft_action_latch(true);

    queue(&mut sim, MissionType::Move);
    sim.mission_host_promote(OWNER, 1, &rules);
    let plane = sim.substrate.entities.get(OWNER).unwrap();
    assert_eq!(
        crate::sim::aircraft::attack_state(plane),
        Some(5),
        "Mission_Attack runs on until Commence"
    );

    // Mission_Attack's state 1 entry clears the latch (`0x00418031`).
    crate::sim::aircraft::attack_mission::enter_attack_state(
        sim.substrate.entities.get_mut(OWNER).unwrap(),
        1,
    );
    sim.mission_host_promote(OWNER, 2, &rules);
    let plane = sim.substrate.entities.get(OWNER).unwrap();
    assert_eq!(
        plane.mission.current(),
        MissionId::from_known(MissionType::Move)
    );
    assert_eq!(plane.mission.handler_state(), 0);
}

/// An Attack commenced over Mission_Move starts Mission_Attack at state 0.
#[test]
fn an_attack_commenced_over_mission_move_starts_at_state_zero() {
    let (mut sim, rules) = fixture(&json!({}));
    set_current(&mut sim, MissionType::Move);
    sim.substrate
        .entities
        .get_mut(OWNER)
        .unwrap()
        .mission
        .set_current_for_test(MissionId::from_known(MissionType::Move), 4);
    queue(&mut sim, MissionType::Attack);
    sim.mission_host_promote(OWNER, 1, &rules);
    let plane = sim.substrate.entities.get(OWNER).unwrap();
    assert_eq!(crate::sim::aircraft::attack_state(plane), Some(0));
}

/// The Move slot `0x004166C0` sends a `Carryall=` type to `0x00416D50`; no
/// retail aircraft type sets the key, so every one runs `0x004166E0`.
#[test]
fn retail_aircraft_types_are_not_carryalls() {
    let Some(rules) = crate::rules::retail_ini_fixture::retail_rulesmd_rules() else {
        return;
    };
    assert!(!rules.aircraft_ids.is_empty());
    for name in &rules.aircraft_ids {
        assert!(!rules.object(name).unwrap().carryall, "{name}");
    }
}

/// Mission_Move's state 0 without a NavCom enters idle mode (`0x004176F0`,
/// `aircraft::idle_entry`) and commences what it picks. On the ground the
/// landed arm drops the destination and the Target and picks Guard (an
/// unarmed aircraft outside a team); in flight the airborne arm sends an
/// aircraft without a weapon to its nearest airfield on Move, keeping its
/// Target.
#[test]
fn mission_move_idle_mode_on_the_ground_and_in_flight() {
    use crate::sim::combat::TargetKind;
    for (z, landed) in [(0, true), (1500, false)] {
        let (mut sim, rules) = fixture(&json!({}));
        set_current(&mut sim, MissionType::Move);
        let plane = sim.substrate.entities.get_mut(OWNER).unwrap();
        plane.locomotor = Some(
            crate::sim::movement::locomotor::LocomotorState::from_object_type(
                rules.object("TEST").unwrap(),
                0,
            ),
        );
        plane.position.set_exact_z_leptons_for_test(Some(z));
        plane
            .mission
            .set_current_for_test(MissionId::from_known(MissionType::Move), 0);
        plane
            .attack_target
            .set_for_test(Some(TargetKind::Cell(70, 70)));
        let airfield = sim.aircraft_nearest_friendly_airfield_cell(OWNER, &rules);

        crate::sim::aircraft::dispatch_mission(&mut sim, OWNER, &rules, Default::default());
        let plane = sim.substrate.entities.get(OWNER).unwrap();
        let expected = if landed {
            MissionType::Guard
        } else {
            MissionType::Move
        };
        assert_eq!(
            plane.mission.current(),
            MissionId::from_known(expected),
            "z {z}"
        );
        assert_eq!(plane.mission.queued(), MissionId::NONE, "z {z}");
        assert_eq!(plane.mission.handler_state(), 0, "z {z}");
        assert_eq!(
            plane.attack_target.target().is_none(),
            landed,
            "z {z}: the Target"
        );
        assert_eq!(
            plane.navigation.nav_com(),
            (!landed).then_some(airfield),
            "z {z}"
        );
    }
}
