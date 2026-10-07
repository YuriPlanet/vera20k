use super::*;
use crate::sim::components::{DriveCoord, NavTargetRef};
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::movement::{FacingClass, locomotor::LocomotorState};

fn no_rules() -> RuleSet {
    RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str("")).unwrap()
}

fn with_mission(actor: &GameEntity, mission: MissionType) -> GameEntity {
    let mut actor = actor.clone();
    actor
        .mission
        .apply_test_fixture(crate::sim::mission::state::MissionTestFixture {
            current: MissionId::from_known(mission),
            queued: MissionId::NONE,
            suspended: MissionId::NONE,
            movement_bypass_latch: 0,
            handler_state: 0,
            mission_start_frame: 0,
            ai_counter: 0,
            dispatch_timer: crate::sim::mission::MissionDispatchTimer::at_frame(0),
        });
    actor
}

/// The Unit receiver's refusals before its coordinate, with both flags set so
/// neither the mission's `Scatter=` nor a NavCom decides (the corpus leaves
/// them to the flags). A refusal draws no RNG and writes nothing.
#[test]
fn unit_refusals_match_native_and_leave_orders_and_rng_untouched() {
    let rows: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/unit_scatter_state.json",
    ))
    .unwrap();
    let rules = no_rules();
    let flags = ScatterFlags::new(true, true);
    let mut checked = 0;
    for row in rows.as_array().unwrap() {
        let input = &row["input"];
        let flag = |key: &str| input[key].as_bool().unwrap_or(false);
        let mission =
            |key: &str, default| MissionId::from_raw(input[key].as_i64().unwrap_or(default) as i32);
        let frame = input["frame"].as_u64().unwrap_or(100) as u32;
        let now = frame.wrapping_add(input["elapsed"].as_u64().unwrap_or(0) as u32);
        let mut actor = GameEntity::test_default(1, "MTNK", "Allies", 5, 5);
        actor.category = EntityCategory::Unit;
        actor
            .mission
            .apply_test_fixture(crate::sim::mission::state::MissionTestFixture {
                current: mission("mission", 5),
                queued: mission("queued", -1),
                suspended: MissionId::NONE,
                movement_bypass_latch: 0,
                handler_state: 0,
                mission_start_frame: 0,
                ai_counter: 0,
                dispatch_timer: crate::sim::mission::MissionDispatchTimer::at_frame(0),
            });
        actor.locomotor = Some(LocomotorState::for_test_kind(if flag("teleport") {
            LocomotorKind::Teleport
        } else {
            LocomotorKind::Drive
        }));
        actor.locomotor.as_mut().unwrap().powered = !flag("power_off");
        actor.set_unit_simple_deploy_for_test(
            flag("deployed"),
            flag("deploying"),
            flag("undeploying"),
        );
        if flag("nav") {
            actor.navigation.nav_com = Some(NavTargetRef::cell(8, 8));
        }
        let mut facing = FacingClass::new(0, input["rot"].as_i64().unwrap_or(5) as i32);
        if flag("turn") {
            facing.set(0x4000, frame);
        }
        actor.body_facing = facing;
        let admitted = unit_scatter_admitted(&actor, flags, &rules, now);
        assert_eq!(admitted, row["admitted"].as_bool().unwrap(), "{input}");
        if !admitted {
            // The production receiver, not only the predicate: a refusal
            // precedes the search, the setter and every write.
            let before = serde_json::to_value(&actor).unwrap();
            let mut sim = Simulation::new();
            sim.session.binary_frame = now;
            sim.substrate.entities.insert(actor);
            let rng = sim.scenario_rng.state();
            assert!(!sim.scatter_null(1, flags, &rules, None).unwrap());
            assert_eq!(
                serde_json::to_value(sim.substrate.entities.get(1).unwrap()).unwrap(),
                before,
                "{input}"
            );
            assert_eq!(sim.scenario_rng.state(), rng, "{input}");
        }
        checked += 1;
    }
    assert_eq!(checked, 69);
}

/// Without the flags the mission's `Scatter=` (`0x00743AD8..0x00743AEA`) and
/// a NavCom (`0x00743B1D..0x00743B2D`) refuse.
#[test]
fn unit_flags_override_mission_scatter_and_nav_com() {
    let rules = RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
        "[Guard]\nScatter=no\n",
    ))
    .unwrap();
    let mut actor = GameEntity::test_default(1, "MTNK", "Allies", 5, 5);
    actor.category = EntityCategory::Unit;
    actor.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
    let guard = with_mission(&actor, MissionType::Guard);
    let hunt = with_mission(&actor, MissionType::Hunt);
    assert!(!unit_scatter_admitted(
        &guard,
        ScatterFlags::new(false, false),
        &rules,
        100
    ));
    assert!(unit_scatter_admitted(
        &guard,
        ScatterFlags::new(true, false),
        &rules,
        100
    ));
    assert!(unit_scatter_admitted(
        &hunt,
        ScatterFlags::new(false, false),
        &rules,
        100
    ));
    let mut nav = hunt.clone();
    nav.navigation.nav_com = Some(NavTargetRef::cell(8, 8));
    assert!(!unit_scatter_admitted(
        &nav,
        ScatterFlags::new(true, false),
        &rules,
        100
    ));
    assert!(unit_scatter_admitted(
        &nav,
        ScatterFlags::new(false, true),
        &rules,
        100
    ));
}

#[test]
fn unit_checks_active_locomotor_and_body_rotation_not_stashed_slot_or_turret() {
    let rules = no_rules();
    let flags = ScatterFlags::new(true, true);
    let mut actor = GameEntity::test_default(1, "CMIN", "Allies", 5, 5);
    actor.category = EntityCategory::Unit;
    let mut actor = with_mission(&actor, MissionType::Move);
    actor.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Teleport));
    assert!(!unit_scatter_admitted(&actor, flags, &rules, 100));
    assert!(crate::sim::movement::locomotor_owner::begin_drive_for_teleporter(&mut actor, 100));
    actor.turret_rotation_latch = true;
    actor.navigation.nav_com = Some(NavTargetRef::cell(8, 8));
    assert!(
        actor
            .locomotor
            .as_mut()
            .unwrap()
            .install_drive_state_for_test(Some(
                crate::sim::movement::DriveLocomotionRuntime::default()
                    .with_head_to_for_test(Some(DriveCoord::cell(6, 5, 0)))
            ))
    );
    assert!(
        unit_scatter_admitted(&actor, flags, &rules, 100),
        "no moving or turret gate, and no-kidding passes the NavCom"
    );
}

/// A pass asks each object once, keeps call order across takes, and never
/// asks again an object it already asked.
#[test]
fn scatter_requests_keep_call_order_and_ask_each_object_once() {
    let mut requests = ScatterRequests::default();
    assert!(requests.request(4, ScatterFlags::new(true, true)));
    assert!(requests.request(2, ScatterFlags::new(true, false)));
    assert!(!requests.request(4, ScatterFlags::new(true, false)));
    assert_eq!(
        requests.take(),
        vec![
            (4, ScatterFlags::new(true, true)),
            (2, ScatterFlags::new(true, false))
        ]
    );
    assert!(!requests.request(2, ScatterFlags::new(true, true)));
    assert!(requests.take().is_empty());
}

/// `UnitClass::Scatter` never asks its locomotor whether it moves, so a
/// driving vehicle is admitted like a parked one.
#[test]
fn unit_receiver_does_not_read_its_own_motion() {
    let mut actor = GameEntity::test_default(1, "MTNK", "Allies", 5, 5);
    actor.category = EntityCategory::Unit;
    let mut actor = with_mission(&actor, MissionType::Move);
    actor.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
    let head = DriveCoord::cell(6, 5, 0);
    actor
        .foot_speed
        .set_speed_fraction(crate::util::fixed_math::SIM_ONE);
    assert!(
        actor
            .locomotor
            .as_mut()
            .unwrap()
            .install_drive_state_for_test(Some(
                crate::sim::movement::DriveLocomotionRuntime::default()
                    .with_destination_for_test(Some(head))
                    .with_head_to_for_test(Some(head))
            ))
    );
    assert_eq!(super::super::motion_query::is_moving(&actor), Some(true));
    assert!(unit_scatter_admitted(
        &actor,
        ScatterFlags::new(true, true),
        &no_rules(),
        100
    ));
}

/// Every class without its own receiver inherits `ObjectClass::Scatter`
/// (`0x005F43A0`, `RET 0xC`): a building takes the call and nothing happens.
#[test]
fn building_receiver_is_the_object_no_op() {
    let building = GameEntity::test_default_of_category(
        1,
        "GAREFN",
        "Allies",
        5,
        5,
        EntityCategory::Structure,
    );
    let before = serde_json::to_value(&building).unwrap();
    let mut sim = Simulation::new();
    sim.substrate.entities.insert(building);
    let rng = sim.scenario_rng.state();
    assert!(
        !sim.scatter_null(1, ScatterFlags::new(true, true), &no_rules(), None)
            .unwrap()
    );
    assert_eq!(
        serde_json::to_value(sim.substrate.entities.get(1).unwrap()).unwrap(),
        before
    );
    assert_eq!(sim.scenario_rng.state(), rng);
}

/// Stock skirmish gate values: `[CombatDamage] PlayerScatter=no`,
/// `[IQ] Scatter=2`.
fn stock_eligibility() -> ScatterEligibility {
    ScatterEligibility {
        player_scatter: false,
        iq_scatter: 2,
    }
}

#[test]
fn scatter_dispatch_gate_matches_the_native_disjunction() {
    let stock = stock_eligibility();
    let techno = |house_iq| {
        Some(ScatterTechno {
            has_scatter_ability: false,
            house_iq,
        })
    };
    // Nothing set: no dispatch.
    assert!(!scatter_dispatch_allowed(stock, false, false, techno(0)));
    // No-kidding (every locomotor blocked-cell caller) always dispatches.
    assert!(scatter_dispatch_allowed(stock, true, false, techno(0)));
    // An elite in the cell releases it.
    assert!(scatter_dispatch_allowed(stock, false, true, techno(0)));
    // An AI house at MaxIQLevels=5 clears [IQ] Scatter=2.
    assert!(scatter_dispatch_allowed(stock, false, false, techno(5)));
    // Exactly at the threshold: `IQ.Scatter <= house.IQ`.
    assert!(scatter_dispatch_allowed(stock, false, false, techno(2)));
    assert!(!scatter_dispatch_allowed(stock, false, false, techno(1)));
}

#[test]
fn scatter_eligibility_defaults_are_the_rules_constructor_values() {
    let defaults = ScatterEligibility::default();
    assert!(!defaults.player_scatter);
    assert_eq!(defaults.iq_scatter, 3);
}

/// The original `Scatter_Objects` dispatch (tools/spatial_oracle/cell_scatter):
/// each row's Techno list through the production walk; a non-Techno, which VERA
/// never lists, through the gate's cell-wide terms alone.
#[test]
fn cell_scatter_dispatch_matches_original_execution() {
    use crate::rules::ini_parser::IniFile;
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/cell_scatter.json",
    ))
    .unwrap();
    let mut checked = 0;
    for row in corpus.as_array().unwrap() {
        let input = &row["input"];
        let objects = input["objects"].as_array().unwrap();
        let flag = |value: &serde_json::Value, key: &str, default: bool| {
            value[key].as_bool().unwrap_or(default)
        };
        let mut ini = format!(
            "[General]\nFixtureOnly=1\n[IQ]\nScatter={}\n[CombatDamage]\nPlayerScatter={}\n[VehicleTypes]\n",
            input["threshold"].as_i64().unwrap_or(2),
            flag(input, "player_scatter", false),
        );
        for (n, object) in objects.iter().enumerate() {
            ini.push_str(&format!("{n}=T{}\n", object["id"]));
        }
        for object in objects {
            let ability = |key| {
                if flag(object, key, false) {
                    "SCATTER"
                } else {
                    ""
                }
            };
            ini.push_str(&format!(
                "[T{}]\nVeteranAbilities={}\nEliteAbilities={}\n",
                object["id"],
                ability("veteran_scatter"),
                ability("elite_scatter"),
            ));
        }
        let rules = RuleSet::from_ini(&IniFile::from_str(&ini)).unwrap();
        let mut interner = StringInterner::new();
        let mut entities = EntityStore::new();
        let mut houses = BTreeMap::new();
        let bridge = flag(input, "bridge", false);
        let mut technos = Vec::new();
        let mut others = Vec::new();
        for object in objects
            .iter()
            .filter(|object| flag(object, "bridge", false) == bridge)
        {
            let id = object["id"].as_u64().unwrap();
            if !flag(object, "techno", true) {
                others.push(id);
                continue;
            }
            let owner = interner.intern(&format!("H{id}"));
            let kind = interner.intern(&format!("T{id}"));
            let mut entity = GameEntity::new_at_frame_zero_for_test(
                id,
                5,
                5,
                0,
                0,
                owner,
                crate::sim::components::Health { current: 100 },
                kind,
                EntityCategory::Unit,
                0,
                5,
                true,
            );
            entity.set_veterancy_rank(object["rank"].as_u64().unwrap_or(0) as u16 * 100);
            let mut house = HouseState::new(owner, 0, None, true, 0, 10);
            house.current_iq = object["iq"].as_i64().unwrap_or(0) as i32;
            houses.insert(owner, house);
            entities.insert(entity);
            technos.push(id);
        }
        let expected: Vec<u64> = row["dispatch"]
            .as_array()
            .unwrap()
            .iter()
            .map(|id| id.as_u64().unwrap())
            .collect();
        let no_kidding = flag(input, "dispatch_all", false);
        let dispatched = scatter_objects_admitted(
            &technos,
            no_kidding,
            Some(&rules),
            &entities,
            &houses,
            &interner,
        );
        let expected_technos: Vec<u64> = expected
            .iter()
            .copied()
            .filter(|id| technos.contains(id))
            .collect();
        assert_eq!(dispatched, expected_technos, "{}", input["name"]);
        let elite = technos
            .iter()
            .any(|&id| entities.get(id).unwrap().veterancy() >= ELITE_VETERANCY);
        for id in others {
            assert_eq!(
                scatter_dispatch_allowed(
                    ScatterEligibility::from_rules(Some(&rules)),
                    no_kidding,
                    elite,
                    None
                ),
                expected.contains(&id),
                "{}",
                input["name"]
            );
        }
        checked += 1;
    }
    assert_eq!(checked, 62);
}

/// `Scatter_Objects`' unforced walk (0x00481670) on live houses and parsed
/// abilities: a house under `[IQ] Scatter=` keeps its man standing, a
/// SCATTER veteran ability or an elite occupant releases the cell, and so
/// does `PlayerScatter=`. The walk draws no RNG: it takes no generator.
#[test]
fn unforced_walk_reads_house_iq_ability_elite_and_player_scatter() {
    use crate::rules::ini_parser::IniFile;
    let rules = |player_scatter: bool| {
        RuleSet::from_ini(&IniFile::from_str(&format!(
            "[General]\nFixtureOnly=1\n[IQ]\nScatter=2\n[CombatDamage]\nPlayerScatter={player_scatter}\n\
             [InfantryTypes]\n0=E1\n[E1]\nVeteranAbilities=SCATTER\n"
        )))
        .unwrap()
    };
    let stock = rules(false);
    let mut entities = EntityStore::new();
    let mut man = GameEntity::test_default(2, "E1", "Soviet", 5, 5);
    man.category = EntityCategory::Infantry;
    let owner = man.owner();
    entities.insert(man);
    let mut houses = BTreeMap::new();
    houses.insert(owner, HouseState::new(owner, 0, None, true, 0, 10));
    let interner = crate::sim::intern::test_interner();
    let walk = |entities: &EntityStore, houses: &BTreeMap<_, _>, rules: &RuleSet, cell: &[u64]| {
        scatter_objects_admitted(cell, false, Some(rules), entities, houses, &interner)
    };
    for (iq, rank, expected) in [(1, 0, false), (2, 0, true), (1, 100, true)] {
        houses.get_mut(&owner).unwrap().current_iq = iq;
        entities.get_mut(2).unwrap().set_veterancy_rank(rank);
        assert_eq!(
            walk(&entities, &houses, &stock, &[2]),
            if expected { vec![2] } else { vec![] },
            "IQ={iq} rank={rank}"
        );
    }
    houses.get_mut(&owner).unwrap().current_iq = 0;
    entities.get_mut(2).unwrap().set_veterancy_rank(0);
    assert_eq!(walk(&entities, &houses, &rules(true), &[2]), vec![2]);
    let mut elite = GameEntity::test_default(3, "E1", "Soviet", 5, 5);
    elite.category = EntityCategory::Infantry;
    elite.set_veterancy_rank(200);
    entities.insert(elite);
    assert_eq!(walk(&entities, &houses, &stock, &[3, 2]), vec![3, 2]);
    assert!(walk(&entities, &houses, &stock, &[2]).is_empty());
}

/// A Hover Unit's null arm reaches its Hover route, and a Jumpjet
/// Unit's reaches its air destination: the Unit setter sends every retail
/// Unit locomotor to the nearby passable cell, which may be its own cell,
/// and neither arm draws.
#[test]
fn unit_null_arm_moves_hover_and_jumpjet_units() {
    use crate::rules::ini_parser::IniFile;
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=ROBO\n1=DISK\n\
         [ROBO]\nStrength=200\nSpeed=6\nSpeedType=Hover\nMovementZone=AmphibiousDestroyer\n\
         Locomotor={4A582742-9839-11d1-B709-00A024DDAFD1}\n\
         [DISK]\nStrength=200\nSpeed=6\nSpeedType=Hover\nMovementZone=Fly\n\
         Locomotor={92612C46-F71F-11d1-AC9F-006008055BB5}\nJumpjetHeight=500\n",
    ))
    .unwrap();
    let mut sim = Simulation::new();
    crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
    let hover = sim
        .spawn_object("ROBO", "Americans", 10, 10, 0, &rules)
        .unwrap();
    let jumpjet = sim
        .spawn_object("DISK", "Americans", 20, 20, 0, &rules)
        .unwrap();
    for (id, kind) in [
        (hover, LocomotorKind::Hover),
        (jumpjet, LocomotorKind::Jumpjet),
    ] {
        let actor = sim.substrate.entities.get(id).unwrap();
        assert_eq!(actor.locomotor.as_ref().unwrap().active_kind(), kind);
        assert!(actor.movement_target.is_none() && actor.navigation.nav_com.is_none());
    }
    let rng = sim.scenario_rng.state();
    let flags = ScatterFlags::new(true, true);
    let cell = sim.scatter_nearby_cell(hover, &rules).unwrap();
    assert!(!sim.scatter_null(hover, flags, &rules, None).unwrap());
    let unit = sim.substrate.entities.get(hover).unwrap();
    let route = unit.movement_target.as_ref().expect("the Hover route");
    assert_eq!(route.final_goal, Some(cell));
    assert_eq!((unit.position.rx, unit.position.ry), (10, 10));
    let cell = sim.scatter_nearby_cell(jumpjet, &rules).unwrap();
    assert!(!sim.scatter_null(jumpjet, flags, &rules, None).unwrap());
    let unit = sim.substrate.entities.get(jumpjet).unwrap();
    assert_eq!(
        unit.navigation.nav_com,
        Some(NavTargetRef::cell(cell.0, cell.1)),
        "the Jumpjet's setter writes its NavCom"
    );
    assert!(unit.movement_target.is_some());
    assert_eq!(sim.scenario_rng.state(), rng);
}

/// The Unit null arm (tools/spatial_oracle/unit_null_scatter) executes
/// Foot+4C, the nearby-cell search from its cell and `SetDestination(cell,
/// 1)` for any answer but NullCell, with no draw, no queued mission and no
/// Process, whatever the flags that passed its gates. The production
/// receiver on a Drive unit spawned on land finds a cell; for the NullCell
/// row it stands on a map without a native Size, where the search answers
/// nothing, and nothing about it changes.
#[test]
fn unit_null_arm_matches_original_execution() {
    use crate::rules::ini_parser::IniFile;
    let rows: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/unit_null_scatter.json",
    ))
    .unwrap();
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=MTNK\n[MTNK]\nStrength=200\nSpeed=6\nSpeedType=Track\n\
         Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n",
    ))
    .unwrap();
    let mut checked = 0;
    for row in rows.as_array().unwrap() {
        let input = &row["input"];
        let found = !row["destination"].is_null();
        let calls = ["coordinate", "fnpc", "destination"];
        let calls = if found { &calls[..] } else { &calls[..2] };
        assert_eq!(row["events"], serde_json::json!(calls), "{input}");
        if found {
            assert_eq!(row["destination"], serde_json::json!([input["answer"], 1]));
        }
        assert_eq!(
            row["random_indices"][0], row["random_indices"][1],
            "{input}"
        );
        let flag = |i: usize| input["flags"][i].as_i64().unwrap_or(1) == 1;
        let flags = ScatterFlags::new(flag(0), flag(1));
        let mut sim = Simulation::new();
        let unit = if found {
            crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
            let at = |i: usize| input["actor"][i].as_i64().unwrap_or(2688) as u16 / 256;
            sim.spawn_object("MTNK", "Americans", at(0), at(1), 0, &rules)
                .unwrap()
        } else {
            let mut actor = GameEntity::test_default(1, "MTNK", "Allies", 10, 10);
            actor.category = EntityCategory::Unit;
            actor.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
            sim.substrate
                .entities
                .insert(with_mission(&actor, MissionType::Guard));
            sim.interner = crate::sim::intern::test_interner();
            1
        };
        let nav = sim.foot_navigation_coordinate(unit).unwrap();
        assert_eq!(
            serde_json::json!([nav.x / 256, nav.y / 256]),
            row["fnpc_seed"]
        );
        let cell = sim.scatter_nearby_cell(unit, &rules);
        assert_eq!(cell.is_some(), found, "{input}");
        let before = serde_json::to_value(sim.substrate.entities.get(unit).unwrap()).unwrap();
        let rng = sim.scenario_rng.state();
        assert!(!sim.scatter_null(unit, flags, &rules, None).unwrap());
        let unit = sim.substrate.entities.get(unit).unwrap();
        match cell {
            Some((x, y)) => {
                assert_eq!(unit.navigation.nav_com, Some(NavTargetRef::cell(x, y)));
                assert_eq!(unit.mission.queued(), MissionId::NONE, "{input}");
            }
            None => assert_eq!(serde_json::to_value(unit).unwrap(), before, "{input}"),
        }
        assert_eq!(sim.scenario_rng.state(), rng, "{input}");
        checked += 1;
    }
    assert_eq!(checked, 4);
}
