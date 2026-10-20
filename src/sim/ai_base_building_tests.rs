use super::*;
use crate::rules::ini_parser::IniFile;
use crate::sim::house_state::HouseState;
use crate::sim::power_system::PowerState;
use crate::sim::rng::SimRng;
use serde_json::Value;

fn oracle() -> &'static Value {
    crate::test_fixture::json("tools/ai_base_building_oracle.json")
}

fn int(value: &Value) -> i64 {
    value.as_i64().unwrap()
}

fn flag(value: &Value) -> bool {
    value.as_bool().unwrap()
}

#[test]
fn suggest_new_object_matches_original_raw_factory_dispatch_table() {
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/trigger_no_factories_events.json",
    ))
    .unwrap();
    let dispatch = &corpus["factory"]["suggest_dispatch"];
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[BuildingTypes]\n0=B\n[VehicleTypes]\n0=U\n[InfantryTypes]\n0=I\n[AircraftTypes]\n0=A\n[B]\n[U]\n[I]\n[A]\n",
    )).unwrap();
    let mut sim = Simulation::with_seed(31);
    let owner = sim.interner.intern("AI");
    let mut house = HouseState::new(owner, 0, None, false, 1000, 10);
    house.ai_production_mut().set_for_test(0, 0, true);
    for kind in [
        UnitChoiceKind::Unit,
        UnitChoiceKind::Infantry,
        UnitChoiceKind::Aircraft,
    ] {
        house.ai_unit_choices_mut().set(kind, 0);
    }
    sim.houses.register(house);
    // Original jump targets load the existing four House choices. The saved
    // byte table establishes aliases; this is control-flow/data evidence,
    // not an execution of a complete native production queue.
    for row in corpus["factory"]["rtti_name_table"].as_array().unwrap() {
        let rtti = int(&row["value"]) as i32;
        let entry = dispatch["aliases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|alias| int(&alias["rtti"]) == i64::from(rtti))
            .map_or(int(&dispatch["default_entry"]), |alias| {
                int(&alias["entry"])
            });
        let expected = match entry {
            0x4fbddc => Some("B"),
            0x4fbdaf => Some("U"),
            0x4fbdc6 => Some("I"),
            0x4fbd99 => Some("A"),
            0x4fbdf3 => None,
            _ => panic!("unreviewed native jump target {entry:x}"),
        };
        assert_eq!(
            suggest_new_object(&sim, &rules, owner, rtti).map(|object| object.id.as_str()),
            expected,
            "{row}"
        );
    }
}

/// The oracle's BuildingTypes, in its array order, then the economy's lists
/// and the walls' types (the oracle's 13 and 14).
fn rules(advanced_prerequisite: &str) -> RuleSet {
    let text = format!(
        "[General]\nWallTower=WALLTOWER\nGDIPowerPlant=GPOWER\nNodRegularPower=NPOWER\n\
         NodAdvancedPower=NAPOWER\nThirdPowerPlant=TPOWER\nAIAlternateProductionCreditCutoff=2000\n\
         AIPickWallDefensePercent=30,50,70\n\
         [AI]\nBuildConst=YARD\nBuildBarracks=BARRA,BARRB\nBuildWeapons=WEAPA,WEAPB\n\
         ConcreteWalls=WALL\n\
         [InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n\
         [BuildingTypes]\n0=GPOWER\n1=NPOWER\n2=NAPOWER\n3=TPOWER\n4=YARD\n5=DRAINER\n\
         6=WALLTOWER\n7=DOCK\n8=PLAIN\n9=BARRA\n10=BARRB\n11=WEAPA\n12=WEAPB\n\
         13=WALL\n14=GUARDED\n\
         [GPOWER]\nPower=100\n[NPOWER]\nPower=100\n[NAPOWER]\nPower=200\n{advanced_prerequisite}\n\
         [TPOWER]\nPower=100\n[YARD]\nConstructionYard=yes\nPower=-50\n[DRAINER]\nPower=-50\n\
         [WALLTOWER]\n[DOCK]\nNaval=yes\n[PLAIN]\n[BARRA]\n[BARRB]\n[WEAPA]\n[WEAPB]\n\
         [WALL]\nWall=yes\n[GUARDED]\nStrength=500\nProtectWithWall=yes\n"
    );
    RuleSet::from_ini_with_fixed_art_for_test(
        &crate::sim::arena_fixture::clear_ground_ini(&text),
        &IniFile::from_str("[GUARDED]\nFoundation=2x2\n"),
    )
    .unwrap()
}

fn computer_house(game_mode_nonzero: bool) -> (Simulation, InternedId) {
    let mut sim = Simulation::new();
    sim.session.set_game_mode(
        crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(game_mode_nonzero),
    );
    let owner = sim.interner.intern("AIHouse");
    let country = sim.interner.intern("Americans");
    sim.houses
        .register(HouseState::new(owner, 0, Some(country), false, 10_000, 10));
    (sim, owner)
}

/// Checks the native draws (stream `Scenario+0x218`, the range) and returns
/// the stream the port must leave: `sim`'s, advanced by those draws.
fn expect_draws(sim: &mut Simulation, draws: &[&Value], low: i32, high: i32) -> SimRng {
    if let Some(first) = draws.first() {
        sim.scenario_rng = SimRng::answering(low, high, int(&first[4]) as i32);
    }
    let mut expected = sim.scenario_rng.clone();
    for draw in draws {
        assert_eq!(
            [int(&draw[1]), int(&draw[2]), int(&draw[3])],
            [0x218, i64::from(low), i64::from(high)]
        );
        expected.next_range_i32_inclusive(low, high);
    }
    expected
}

#[test]
fn the_production_mode_steps_as_native() {
    let oracle = oracle();
    let rules = rules("");
    assert_eq!(rules.general.ai_alternate_production_credit_cutoff, 2000);
    let rows = oracle["economy"].as_array().unwrap();
    for row in rows {
        assert_eq!(int(&row["cutoff"]), 2000);
        let (mut sim, owner) = computer_house(int(&row["game_mode"]) != 0);
        // The house owns the second type of each list (the oracle's too).
        let owned: Vec<(InternedId, i32)> = [("BARRB", "barracks"), ("WEAPB", "weapons")]
            .into_iter()
            .filter(|(_, list)| flag(&row[*list]))
            .map(|(name, _)| (sim.interner.intern(name), 1))
            .collect();
        let house = sim.houses.get_mut(&owner).unwrap();
        house.set_is_human_for_test(flag(&row["human"]));
        house.set_player_control_for_test(flag(&row["control"]));
        house
            .economy_mut()
            .set_credits_for_test(int(&row["money"]) as i32);
        house
            .ai_production_mut()
            .set_for_test(int(&row["mode"]) as i32, -1, true);
        house.tracking_mut().set_for_test(0, &[], (0, 0, 0), &owned);
        let mut power_state = PowerState::default();
        power_state.set_total_output_for_test(int(&row["output"]) as i32);
        power_state.set_total_drain_for_test(int(&row["drain"]) as i32);
        sim.power_states
            .map_mut_for_test()
            .insert(owner, power_state);
        let draws: Vec<&Value> = row["draws"].as_array().unwrap().iter().collect();
        let expected = expect_draws(&mut sim, &draws, 0, 1);

        economy_state_machine(&mut sim, &rules, owner, int(&row["kind"]) == 6);

        assert_eq!(
            i64::from(sim.houses[&owner].ai_production().mode()),
            int(&row["mode_after"]),
            "{row}"
        );
        assert_eq!(
            sim.scenario_rng.logical_state(),
            expected.logical_state(),
            "{row}"
        );
    }
    // Mode 2 below the cutoff with both lists owned and enough power: two
    // kinds, two answers, output above or equal to the drain.
    assert_eq!(
        rows.iter()
            .filter(|row| !row["draws"].as_array().unwrap().is_empty())
            .count(),
        8
    );
}

#[test]
fn the_building_choice_handles_nodes_as_native() {
    let oracle = oracle();
    for row in oracle["chooser"].as_array().unwrap() {
        let prerequisite = match row["advanced_prereqs"].as_array().unwrap().as_slice() {
            [] => "",
            [index] => match int(index) {
                4 => "Prerequisite=YARD",
                -1 => "Prerequisite=POWER",
                -6 => "Prerequisite=PROC",
                other => panic!("no fixture for prerequisite {other}"),
            },
            more => panic!("no fixture for prerequisites {more:?}"),
        };
        let rules = rules(prerequisite);
        let (mut sim, owner) = computer_house(true);
        // `0x0042E820`'s answers: the house's buildings on those nodes.
        let node_buildings = row["node_buildings"].as_array().unwrap();
        if !node_buildings.is_empty() {
            crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
        }
        for building in node_buildings {
            let node = &row["nodes"][int(&building[0]) as usize];
            let ty = rules.building_type_at(int(&node[0]) as i32).unwrap();
            let (x, y) = (int(&building[1]) / 256, int(&building[2]) / 256);
            sim.spawn_object(&ty.id, "AIHouse", x as u16, y as u16, 0, &rules)
                .unwrap();
        }
        let house = sim.houses.get_mut(&owner).unwrap();
        house.ai_production_mut().set_for_test(
            0,
            int(&row["choice"]) as i32,
            flag(&row["naval_allowed"]),
        );
        house.set_build_const_order_for_test(
            (0..int(&row["yards"]) as u64).map(|id| 1000 + id).collect(),
        );
        house.set_side_index_for_test(int(&row["side"]) as i32);
        house.set_difficulty_for_test(
            crate::sim::house_state::HouseDifficulty::from_native(int(&row["difficulty"]) as i32)
                .unwrap(),
        );
        house.base_plan_mut().set_nodes_for_test(
            row["nodes"]
                .as_array()
                .unwrap()
                .iter()
                .map(|node| BasePlanNode {
                    type_or_control: int(&node[0]) as i32,
                    packed_cell: pack_base_plan_cell(int(&node[1]) as i32, int(&node[2]) as i32),
                    filled: node.get(3).is_some_and(|filled| int(filled) != 0),
                    retry_count: 0,
                })
                .collect(),
        );
        let mut power_state = PowerState::default();
        power_state.set_total_output_for_test(int(&row["output"]) as i32);
        power_state.set_total_drain_for_test(int(&row["drain"]) as i32);
        power_state.start_blackout(0, int(&row["blackout"]) as i32);
        power_state.set_has_drained_power_source_for_test(flag(&row["drained_source"]));
        sim.power_states
            .map_mut_for_test()
            .insert(owner, power_state);
        let draws: Vec<&Value> = row["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|event| event[0] == "draw")
            .collect();
        let expected = expect_draws(&mut sim, &draws, 0, 99);

        choose_building(&mut sim, &rules, owner, None);

        let label = row["label"].as_str().unwrap();
        let house = &sim.houses[&owner];
        assert_eq!(
            i64::from(house.ai_production().building_choice()),
            int(&row["choice_after"]),
            "{label}"
        );
        let nodes: Vec<[i64; 5]> = house
            .base_plan()
            .nodes()
            .iter()
            .map(|node| {
                let (x, y) = unpack_base_plan_cell(node.packed_cell);
                [
                    i64::from(node.type_or_control),
                    i64::from(x),
                    i64::from(y),
                    i64::from(node.filled),
                    i64::from(node.retry_count),
                ]
            })
            .collect();
        let native: Vec<[i64; 5]> = row["nodes_after"]
            .as_array()
            .unwrap()
            .iter()
            .map(|node| std::array::from_fn(|field| int(&node[field])))
            .collect();
        assert_eq!(nodes, native, "{label}");
        assert_eq!(
            sim.scenario_rng.logical_state(),
            expected.logical_state(),
            "{label}"
        );
    }
}

/// The walls of `0x0050C340` for the oracle's rows: the wall type through
/// the production rules read, the nodes through [`wall_nodes`] answering
/// `0x0042E820` with the row's buildings.
#[test]
fn the_walls_match_native() {
    let oracle = oracle();
    // name, AIBasePlanningSide, ProtectWithWall, foundation, bib, native
    // width, native height.
    let types = oracle["wall_types"].as_array().unwrap();
    let rules_with = |walls: &[&str]| {
        let mut text = format!(
            "[AI]\nConcreteWalls={}\n[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n\
             [BuildingTypes]\n",
            walls.join(",")
        );
        for (index, ty) in types.iter().enumerate() {
            text += &format!("{index}={}\n", ty[0].as_str().unwrap());
        }
        let mut art_text = String::new();
        for ty in types {
            let foundation = crate::rules::foundation::FOUNDATION_TABLE[int(&ty[3]) as usize].name;
            text += &format!(
                "[{}]\nAIBasePlanningSide={}\nProtectWithWall={}\n",
                ty[0].as_str().unwrap(),
                int(&ty[1]),
                flag(&ty[2])
            );
            // The native corpus supplies the Foundation index in type memory.
            art_text += &format!("[{}]\nFoundation={foundation}\n", ty[0].as_str().unwrap());
        }
        RuleSet::from_ini_with_fixed_art_for_test(
            &IniFile::from_str(&text),
            &IniFile::from_str(&art_text),
        )
        .unwrap()
    };
    // The native Width and Height tables (`0x008192B8`, `0x00819310`) that
    // `0x0045EC90` and `0x0045ECA0(0)` read, for every foundation; the
    // foundation rows below execute both inside `AI_BuildWalls`. VERA's size
    // has no bib term: `Height(0)` skips the bib branch, which only the
    // oracle's bib rows exercise.
    let all = rules_with(&[]);
    for ty in types {
        let name = ty[0].as_str().unwrap();
        assert_eq!(
            crate::sim::ai_base_site::foundation_size(all.object(name).unwrap()),
            (int(&ty[5]) as i32, int(&ty[6]) as i32),
            "{name}"
        );
    }
    let mut placed = 0;
    for row in oracle["walls"].as_array().unwrap() {
        let label = row["label"].as_str().unwrap();
        let walls: Vec<&str> = row["walls"]
            .as_array()
            .unwrap()
            .iter()
            .map(|index| types[int(index) as usize][0].as_str().unwrap())
            .collect();
        let rules = rules_with(&walls);
        // A HouseType side of -1 matches only a -1 type, as no u8 side does.
        let wall = wall_type(&rules, int(&row["side"]) as i32);

        let node = |value: &Value| BasePlanNode {
            type_or_control: int(&value[0]) as i32,
            packed_cell: pack_base_plan_cell(int(&value[1]) as i32, int(&value[2]) as i32),
            filled: false,
            retry_count: 0,
        };
        let mut plan = crate::sim::base_plan::BasePlanState::default();
        plan.set_nodes_for_test(row["nodes"].as_array().unwrap().iter().map(node).collect());
        let nodes = plan.nodes();
        let buildings = row["node_buildings"].as_array().unwrap();
        // `0x0042E820`'s answers, the row's buildings: the node's cell always
        // equals the building's (`0x0041BEA0`, Location / 256), so these
        // rows' Location offsets only move where inside the cell it stands.
        let walled = wall_nodes(nodes, int(&row["index"]) as usize, wall, |at| {
            let building = buildings
                .iter()
                .find(|building| int(&building[0]) as usize == at)?;
            let ty = rules.building_type_at(nodes[at].type_or_control)?;
            let cell = (
                (int(&building[1]) / 256) as i16,
                (int(&building[2]) / 256) as i16,
            );
            ty.protect_with_wall
                .then(|| (cell, crate::sim::ai_base_site::foundation_size(ty)))
        });
        assert_eq!(i64::from(walled.is_some()), int(&row["result"]), "{label}");
        if let Some((at, walls)) = walled {
            for wall in walls {
                plan.insert_after(at, wall);
            }
            placed += 1;
        }
        let after: Vec<BasePlanNode> = row["nodes_after"]
            .as_array()
            .unwrap()
            .iter()
            .map(node)
            .collect();
        assert_eq!(plan.nodes(), after, "{label}");
    }
    assert!(placed >= 40, "{placed} rows walled");
}

#[test]
fn the_placement_retry_wait_matches_native() {
    let oracle = oracle();
    let mut general = rules("").general;
    for row in oracle["placement_delay"].as_array().unwrap() {
        general.placement_delay = f64::from_bits(row["minutes_bits"].as_u64().unwrap());
        assert_eq!(
            i64::from(general.placement_delay_frames()),
            int(&row["frames"]),
            "{row}"
        );
    }
}

/// The dormant branches the module residuals name: no retail BuildingType
/// sets `CloakGenerator=` (production reader) or `PowersUpBuilding=` (not
/// parsed; any value in the section).
#[test]
fn retail_building_types_set_no_cloak_generator_or_upgrade() {
    let Some(ini) = crate::rules::retail_ini_fixture::retail_ini("rulesmd.ini") else {
        return;
    };
    let rules =
        crate::rules::retail_ini_fixture::retail_rulesmd_rules().expect("retail rulesmd.ini");
    assert!(rules.building_ids.len() > 100);
    for name in &rules.building_ids {
        let ty = rules.object(name).unwrap();
        assert!(!ty.cloak_generator, "{name}");
        assert!(
            ini.section(name)
                .is_none_or(|section| section.get_for_test("PowersUpBuilding").is_none()),
            "{name}"
        );
    }
}

/// The retail values that decide the walls, through the production reader:
/// the percent by difficulty, and each playable side's wall type (the
/// `ConcreteWalls=` value and GAFWLL's section header both carry comments).
/// No other retail INI gamemd reads sets either key.
#[test]
fn retail_walls_by_difficulty_and_side() {
    let Some(rules) = crate::rules::retail_ini_fixture::retail_rulesmd_rules() else {
        return;
    };
    assert_eq!(rules.general.ai_pick_wall_defense_percent, [50, 25, 10]);
    assert_eq!(rules.concrete_wall_types, ["GAWALL", "NAWALL", "GAFWLL"]);
    for (side, name) in [(0, "GAWALL"), (1, "NAWALL"), (2, "GAFWLL")] {
        assert_eq!(
            wall_type(&rules, side),
            rules.building_type_index(name).unwrap(),
            "side {side}"
        );
    }
}

const EXIT_RULES: &str = "[General]\nAIAlternateProductionCreditCutoff=2000\n\
    MaximumBuildingPlacementFailures=2\n\
    [InfantryTypes]\n[AircraftTypes]\n[VehicleTypes]\n0=TANK\n\
    [BuildingTypes]\n0=YARD\n1=PLAIN\n\
    [YARD]\nStrength=1000\nConstructionYard=yes\n\
    [PLAIN]\nStrength=1000\n\
    [TANK]\nStrength=300\nSpeed=5\nLocomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\
    [Clear]\nBuildable=yes\n";

/// A skirmish computer house with a yard at (12, 12) and one plan node, PLAIN
/// at (16, 16) with its foundation reserved, inside the fixture's
/// `In_Bounds` diamond; the finished PLAIN waits in limbo. Returns the yard
/// and the product.
fn exit_fixture() -> (Simulation, RuleSet, InternedId, u64, u64) {
    let rules = RuleSet::from_ini_with_fixed_art_for_test(
        &IniFile::from_str(EXIT_RULES),
        &IniFile::from_str("[YARD]\nFoundation=2x2\n[PLAIN]\nFoundation=2x2\n"),
    )
    .unwrap();
    let mut sim = Simulation::new();
    crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    let owner = sim.interner.intern("AIHouse");
    let country = sim.interner.intern("Americans");
    sim.houses
        .register(HouseState::new(owner, 0, Some(country), false, 10_000, 10));
    let yard = sim
        .spawn_object("YARD", "AIHouse", 12, 12, 0, &rules)
        .unwrap();
    let plain = rules.building_type_index("PLAIN").unwrap();
    let house = sim.houses.get_mut(&owner).unwrap();
    house.base_plan_mut().set_nodes_for_test(vec![BasePlanNode {
        type_or_control: plain,
        packed_cell: pack_base_plan_cell(16, 16),
        filled: false,
        retry_count: 0,
    }]);
    for (x, y) in [(16, 16), (17, 16), (16, 17), (17, 17)] {
        sim.substrate
            .base_reservations
            .reserve(sim.resolved_terrain.as_ref(), x, y, 0);
    }
    let product = sim
        .construct_object_limbo_at_height(
            "PLAIN",
            "AIHouse",
            0,
            0,
            0,
            0,
            &rules,
            crate::sim::world::FrameEffects::default(),
        )
        .unwrap();
    (sim, rules, owner, yard, product)
}

fn node_state(sim: &Simulation, owner: InternedId) -> Vec<((i16, i16), i32)> {
    sim.houses[&owner]
        .base_plan()
        .nodes()
        .iter()
        .map(|node| (unpack_base_plan_cell(node.packed_cell), node.retry_count))
        .collect()
}

#[test]
fn a_computer_yard_places_its_building_on_the_node_cell() {
    let (mut sim, rules, owner, yard, product) = exit_fixture();
    let plain = rules.building_type_index("PLAIN").unwrap();
    let house = sim.houses.get_mut(&owner).unwrap();
    house.economy_mut().set_credits_for_test(100);
    house.ai_production_mut().set_for_test(0, plain, true);

    let exit = exit_building(
        &mut sim,
        &rules,
        yard,
        product,
        None,
        crate::sim::world::FrameEffects::default(),
    );

    assert_eq!(exit, BuildingExit::Placed);
    let building = sim.substrate.entities.get(product).unwrap();
    assert!(!building.lifecycle.in_limbo());
    assert_eq!((building.position.rx(), building.position.ry()), (16, 16));
    assert!(building.building_up(), "the placed building builds up");
    let house = &sim.houses[&owner];
    // Below the cutoff a building exit moves mode 0 to 2, and the choice is
    // forgotten.
    assert_eq!(house.ai_production().mode(), 2);
    assert_eq!(house.ai_production().building_choice(), -1);
    assert_eq!(node_state(&sim, owner), [((16, 16), 0)]);
}

/// The placed building's Unlimbo queues Construction (`TechnoClass::Unlimbo`
/// `0x006F6E2A` -> `0x0044D6BD..0x0044D6D0`) before its DiscoveredBy(owner)
/// (`0x00440E76`), whose computer arm asks Get_Mission (`0x0044D679`) and so
/// leaves it unopened. The exit commences that Construction
/// (`0x00445329..0x0044533F`). From the placement frame on, each of its
/// Updates matches the native computer `route` rows
/// (`tools/spatial_oracle/building_construction.json`), up to the opening.
#[test]
fn a_computer_yards_building_follows_the_native_computer_route() {
    use crate::sim::building_construction::native_route;
    use crate::sim::mission::{MissionId, MissionType};
    let rows = native_route::rows("computer");
    assert_eq!(rows.len(), 6);
    for row in &rows {
        let name = row["input"]["name"].as_str().unwrap();
        let (mut sim, rules, owner, yard, product) = exit_fixture();
        // The native rows run at GameSpeed 0.
        sim.session.game_options_mut_for_test().game_speed = 0;
        let plain = rules.building_type_index("PLAIN").unwrap();
        let house = sim.houses.get_mut(&owner).unwrap();
        house.economy_mut().set_credits_for_test(100);
        house.ai_production_mut().set_for_test(0, plain, true);
        sim.substrate
            .entities
            .get_mut(product)
            .unwrap()
            .bind_building_construction_control(native_route::control(row));

        let exit = exit_building(
            &mut sim,
            &rules,
            yard,
            product,
            None,
            crate::sim::world::FrameEffects::default(),
        );

        assert_eq!(exit, BuildingExit::Placed, "{name}");
        let building = sim.substrate.entities.get(product).unwrap();
        assert!(
            !building.building_art.actually_placed(),
            "{name}: opened at placement"
        );
        assert_eq!(
            building.mission.current(),
            MissionId::from_known(MissionType::Construction),
            "{name}"
        );
        assert_eq!(building.mission.queued(), MissionId::NONE, "{name}");
        let placed_at = sim.session.binary_frame();
        for frame in row["frames"].as_array().unwrap() {
            let now = placed_at + frame["frame"].as_u64().unwrap() as u32;
            sim.session.set_binary_frame_for_test(now);
            sim.object_ai_visit_one(
                product,
                Some(&rules),
                crate::sim::world::ObjectAiCtx::default(),
            );
            let building = sim.substrate.entities.get(product).unwrap();
            if native_route::assert_frame(building, frame, &format!("{name} frame {now}")) {
                break;
            }
        }
    }
}

#[test]
fn a_unit_of_the_house_on_the_site_makes_the_yard_try_later() {
    let (mut sim, rules, owner, yard, product) = exit_fixture();
    let tank = sim
        .spawn_object("TANK", "AIHouse", 17, 17, 0, &rules)
        .unwrap();

    for count in 1..=2 {
        let exit = exit_building(
            &mut sim,
            &rules,
            yard,
            product,
            None,
            crate::sim::world::FrameEffects::default(),
        );
        assert_eq!(exit, BuildingExit::TryLater);
        assert!(
            sim.substrate
                .entities
                .get(product)
                .unwrap()
                .lifecycle
                .in_limbo()
        );
        assert_eq!(node_state(&sim, owner), [((16, 16), count)]);
    }
    assert!(sim.substrate.entities.get(tank).is_some());
    // The third failure passes MaximumBuildingPlacementFailures=2: the node
    // goes.
    let exit = exit_building(
        &mut sim,
        &rules,
        yard,
        product,
        None,
        crate::sim::world::FrameEffects::default(),
    );
    assert_eq!(exit, BuildingExit::TryLater);
    assert!(node_state(&sim, owner).is_empty());
}

#[test]
fn an_enemy_on_the_site_fails_the_exit_and_the_node_forgets_its_cell() {
    let (mut sim, rules, owner, yard, product) = exit_fixture();
    let enemy = sim.interner.intern("Enemy");
    sim.houses
        .register(HouseState::new(enemy, 1, None, false, 10_000, 10));
    sim.spawn_object("TANK", "Enemy", 16, 16, 0, &rules)
        .unwrap();

    let exit = exit_building(
        &mut sim,
        &rules,
        yard,
        product,
        None,
        crate::sim::world::FrameEffects::default(),
    );

    assert_eq!(exit, BuildingExit::Failed);
    assert!(
        sim.substrate
            .entities
            .get(product)
            .unwrap()
            .lifecycle
            .in_limbo()
    );
    assert_eq!(node_state(&sim, owner), [((0, 0), 0)]);
}

#[test]
fn a_human_yard_places_nothing() {
    let (mut sim, rules, owner, yard, product) = exit_fixture();
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .set_is_human_for_test(true);

    let exit = exit_building(
        &mut sim,
        &rules,
        yard,
        product,
        None,
        crate::sim::world::FrameEffects::default(),
    );

    assert_eq!(exit, BuildingExit::Failed);
    assert!(
        sim.substrate
            .entities
            .get(product)
            .unwrap()
            .lifecycle
            .in_limbo()
    );
    assert_eq!(node_state(&sim, owner), [((16, 16), 0)]);
}
