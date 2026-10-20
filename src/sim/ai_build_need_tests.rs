use super::*;
use crate::rules::ini_parser::IniFile;
use crate::sim::base_plan::pack_base_plan_cell;
use crate::sim::house_state::HouseState;

/// A country's economy: a yard (a `BuildingType` factory), a refinery, a war
/// factory, a plain building, a gate, an unsellable building, a harvester
/// and an MCV, all ownable by `Americans`.
const RULES: &str = "[General]\nBaseUnit=MCV\nHarvesterUnit=HARV\n\
    [AI]\nBuildConst=YARD\nBuildRefinery=REFN\nBuildWeapons=WEAP\n\
    [Countries]\n0=Americans\n[Americans]\nSide=GDI\n\
    [InfantryTypes]\n[AircraftTypes]\n[VehicleTypes]\n0=HARV\n1=MCV\n\
    [BuildingTypes]\n0=YARD\n1=REFN\n2=WEAP\n3=PLAIN\n4=GATE\n5=BUNKER\n\
    [YARD]\nStrength=1000\nConstructionYard=yes\nFactory=BuildingType\nCost=3000\n\
    Owner=Americans\n\
    [REFN]\nStrength=1000\nRefinery=yes\nResourceDestination=yes\nCost=2000\nOwner=Americans\n\
    [WEAP]\nStrength=1000\nWeaponsFactory=yes\nFactory=UnitType\nCost=2000\nOwner=Americans\n\
    [PLAIN]\nStrength=1000\nCost=600\nOwner=Americans\n\
    [GATE]\nStrength=1000\nGate=yes\nCost=300\nOwner=Americans\n\
    [BUNKER]\nStrength=1000\nUnsellable=yes\nCost=900\nOwner=Americans\n\
    [HARV]\nStrength=600\nHarvester=yes\nResourceGatherer=yes\nCost=1400\nOwner=Americans\n\
    Speed=4\nLocomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\
    [MCV]\nStrength=1000\nCost=3000\nOwner=Americans\nSpeed=4\n\
    Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\
    [Clear]\nBuildable=yes\n";

fn rules() -> RuleSet {
    rules_from(RULES)
}

fn rules_from(ini: &str) -> RuleSet {
    let mut rules = RuleSet::from_ini(&IniFile::from_str(ini)).unwrap();
    for building in ["YARD", "REFN", "WEAP", "PLAIN", "GATE", "BUNKER"] {
        rules.set_buildup_control_for_test(building, [0, 17, 3]);
    }
    rules
}

/// A skirmish computer house `AIHouse` of `Americans` holding `credits`,
/// with its production on, on flat ground.
fn fixture(credits: i32) -> (Simulation, RuleSet, InternedId) {
    fixture_with(rules(), credits)
}

fn fixture_with(rules: RuleSet, credits: i32) -> (Simulation, RuleSet, InternedId) {
    let mut sim = Simulation::new();
    crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    let owner = sim.interner.intern("AIHouse");
    let country = sim.interner.intern("Americans");
    let mut house = HouseState::new(owner, 0, Some(country), false, credits, 10);
    house.enable_scripted_production();
    sim.houses.register(house);
    (sim, rules, owner)
}

fn spawn(
    sim: &mut Simulation,
    rules: &RuleSet,
    type_id: &str,
    owner: &str,
    cell: (u16, u16),
) -> u64 {
    sim.spawn_object(type_id, owner, cell.0, cell.1, 0, rules)
        .unwrap()
}

fn node(rules: &RuleSet, type_id: &str, cell: (i32, i32)) -> BasePlanNode {
    BasePlanNode {
        type_or_control: rules.building_type_index(type_id).unwrap(),
        packed_cell: pack_base_plan_cell(cell.0, cell.1),
        filled: true,
        retry_count: 0,
    }
}

/// The house's buildings at `cells`, each on its own plan node, in order.
fn plan(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    buildings: &[(&str, (u16, u16))],
) -> Vec<u64> {
    let ids = buildings
        .iter()
        .map(|&(type_id, cell)| spawn(sim, rules, type_id, "AIHouse", cell))
        .collect();
    let nodes = buildings
        .iter()
        .map(|&(type_id, (x, y))| node(rules, type_id, (i32::from(x), i32::from(y))))
        .collect();
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .base_plan_mut()
        .set_nodes_for_test(nodes);
    ids
}

fn set_credits(sim: &mut Simulation, owner: InternedId, credits: i32) {
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .economy_mut()
        .set_credits_for_test(credits);
}

fn selling(sim: &Simulation, id: u64) -> bool {
    sim.substrate.entities.get(id).is_some_and(|building| {
        building.mission.current() == MissionId::from_known(MissionType::Selling)
    })
}

fn hunting(sim: &Simulation, owner: InternedId) -> bool {
    sim.houses[&owner].strategy_emergency().all_to_hunt_bias()
}

fn plan_types(sim: &Simulation, rules: &RuleSet, owner: InternedId) -> Vec<(String, (i16, i16))> {
    sim.houses[&owner]
        .base_plan()
        .nodes()
        .iter()
        .map(|node| {
            let name = rules
                .building_type_at(node.type_or_control)
                .map_or(String::new(), |ty| ty.id.clone());
            (name, unpack_base_plan_cell(node.packed_cell))
        })
        .collect()
}

/// The yard's factory starts a build of `type_id` (`BuildingClass::
/// Factory_AI`'s new factory for the house's building choice).
fn yard_builds(sim: &mut Simulation, rules: &RuleSet, owner: InternedId, yard: u64, type_id: &str) {
    let choice = rules.building_type_index(type_id).unwrap();
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .ai_production_mut()
        .set_for_test(0, choice, true);
    crate::sim::production::factory_ai(sim, rules, yard, None, FrameEffects::default());
    assert!(sim.production.factories().building_factory(yard).is_some());
}

#[test]
fn a_house_with_a_refinery_and_a_miner_needs_nothing() {
    let (mut sim, rules, owner) = fixture(0);
    spawn(&mut sim, &rules, "REFN", "AIHouse", (14, 10));
    spawn(&mut sim, &rules, "HARV", "AIHouse", (20, 20));
    let tracking = sim.houses[&owner].tracking();
    assert_eq!(
        (
            tracking.resource_destinations(),
            tracking.resource_gatherers()
        ),
        (1, 1)
    );
    assert!(!check_build_need(&sim, &rules, owner));
}

/// Without a refinery a yard's house can make money only above the
/// refinery's price (`0x004F680F`, strict), and must rebuild only below it
/// (`0x004FDCBB`): one credit short is a need, the price itself is none.
#[test]
fn a_house_must_rebuild_a_refinery_it_cannot_pay_for() {
    let (mut sim, rules, owner) = fixture(0);
    spawn(&mut sim, &rules, "YARD", "AIHouse", (10, 10));
    for (credits, need) in [(1999, true), (2000, false), (2001, false)] {
        set_credits(&mut sim, owner, credits);
        assert_eq!(check_build_need(&sim, &rules, owner), need, "{credits}");
    }
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .set_is_human_for_test(true);
    set_credits(&mut sim, owner, 0);
    assert!(!check_build_need(&sim, &rules, owner), "a human house");
}

/// A country without a `HarvesterUnit=` type (retail Yuri's) makes money with
/// a gatherer and no refinery (`0x004F67AF`); one with a harvester type and
/// no refinery makes it only above the refinery's price (`0x004F6807`).
#[test]
fn a_country_without_a_harvester_type_earns_with_any_gatherer() {
    for harvester_type in [true, false] {
        let rules = if harvester_type {
            rules()
        } else {
            rules_from(&RULES.replace("HarvesterUnit=HARV\n", ""))
        };
        let (mut sim, rules, owner) = fixture_with(rules, 0);
        spawn(&mut sim, &rules, "YARD", "AIHouse", (10, 10));
        spawn(&mut sim, &rules, "HARV", "AIHouse", (20, 20));
        assert_eq!(
            check_build_need(&sim, &rules, owner),
            harvester_type,
            "harvester type {harvester_type}"
        );
    }
}

/// A refinery the yard is building is waited for while the money covers
/// what its factory still owes (`Factory+0x60`).
#[test]
fn the_need_waits_for_the_refinery_the_yard_is_building() {
    let (mut sim, rules, owner) = fixture(1500);
    let yard = spawn(&mut sim, &rules, "YARD", "AIHouse", (10, 10));
    let refinery = rules.building_type_index("REFN").unwrap();
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .ai_production_mut()
        .set_for_test(0, refinery, true);
    assert!(check_build_need(&sim, &rules, owner), "no factory holds it");
    yard_builds(&mut sim, &rules, owner, yard, "REFN");
    for (owed, need) in [(1000, false), (1500, false), (1501, true)] {
        sim.production
            .factories_mut()
            .test_building_factory_mut(yard)
            .unwrap()
            .balance = owed;
        assert_eq!(check_build_need(&sim, &rules, owner), need, "{owed}");
    }
}

/// With a refinery standing, a harvester the house has chosen is waited for
/// while the money covers what the factory building it still owes
/// (`0x004FDAFF..0x004FDBB2`).
#[test]
fn the_need_waits_for_the_harvester_the_war_factory_is_building() {
    let (mut sim, rules, owner) = fixture(1000);
    spawn(&mut sim, &rules, "REFN", "AIHouse", (14, 10));
    let factory = spawn(&mut sim, &rules, "WEAP", "AIHouse", (20, 10));
    let harvester = rules
        .type_array_index(ObjectCategory::Vehicle, "HARV")
        .unwrap();
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .ai_unit_choices_mut()
        .set(UnitChoiceKind::Unit, harvester);
    assert!(check_build_need(&sim, &rules, owner), "no factory holds it");
    crate::sim::production::factory_ai(&mut sim, &rules, factory, None, FrameEffects::default());
    for (owed, need) in [(1000, false), (1001, true)] {
        sim.production
            .factories_mut()
            .test_building_factory_mut(factory)
            .expect("the war factory builds the harvester")
            .balance = owed;
        assert_eq!(check_build_need(&sim, &rules, owner), need, "{owed}");
    }
}

/// With a refinery on the map the walk over BuildingClass::Array reads as
/// many buildings as the house owns, whoever owns them: another house's
/// refinery building up among the first holds the need off.
#[test]
fn another_houses_refinery_building_up_first_holds_the_need() {
    for first in [true, false] {
        let (mut sim, rules, owner) = fixture(0);
        let (name, country) = (
            sim.interner.intern("Other"),
            sim.interner.intern("Americans"),
        );
        sim.houses
            .register(HouseState::new(name, 0, Some(country), false, 0, 10));
        let other = |sim: &mut Simulation| {
            let id = spawn(sim, &rules, "REFN", "Other", (30, 30));
            sim.mission_assign_exact(
                id,
                MissionId::from_known(MissionType::Construction),
                sim.session.binary_frame(),
            )
            .unwrap();
        };
        if first {
            other(&mut sim);
        }
        spawn(&mut sim, &rules, "YARD", "AIHouse", (10, 10));
        spawn(&mut sim, &rules, "REFN", "AIHouse", (14, 10));
        if !first {
            other(&mut sim);
        }
        // No miner and 0 credits: the harvester's price is beyond them.
        assert_eq!(
            check_build_need(&sim, &rules, owner),
            !first,
            "first {first}"
        );
    }
}

/// The rebuild sells from the last node down until the balance and the
/// refunds to come exceed the refinery's price, abandons the house's
/// production and plans the refinery at the first open node.
#[test]
fn the_rebuild_sells_from_the_last_node_until_it_can_pay() {
    let (mut sim, rules, owner) = fixture(1000);
    let ids = plan(
        &mut sim,
        &rules,
        owner,
        &[
            ("YARD", (10, 10)),
            ("PLAIN", (14, 10)),
            ("PLAIN", (18, 10)),
            ("PLAIN", (22, 10)),
        ],
    );
    // The lost refinery's node, never filled, is the first open node.
    let lost = BasePlanNode {
        filled: false,
        ..node(&rules, "REFN", (30, 10))
    };
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .base_plan_mut()
        .nodes_mut_for_test()
        .insert(1, lost);
    yard_builds(&mut sim, &rules, owner, ids[0], "PLAIN");
    let held = sim
        .production
        .factories()
        .building_factory(ids[0])
        .and_then(|factory| factory.object.as_ref())
        .and_then(|object| object.entity_id)
        .unwrap();
    assert!(check_build_need(&sim, &rules, owner));

    manage_build_queue(&mut sim, &rules, owner, None, FrameEffects::default());

    // 1000 + 600 is not above 2000; 1000 + 1200 is.
    let sold: Vec<bool> = ids.iter().map(|&id| selling(&sim, id)).collect();
    assert_eq!(sold, [false, false, true, true]);
    assert!(
        sim.production
            .factories()
            .building_factory(ids[0])
            .is_none()
    );
    assert!(
        sim.substrate.entities.get(held).is_none(),
        "the held object goes"
    );
    // Nothing was paid on the build, so its abandon refunds nothing.
    assert_eq!(sim.houses[&owner].economy().credits(), 1000);
    let house = &sim.houses[&owner];
    assert_eq!(
        house.ai_production().building_choice(),
        rules.building_type_index("REFN").unwrap()
    );
    assert_eq!(house.ai_production().mode(), 1);
    assert_eq!(
        plan_types(&sim, &rules, owner),
        [
            ("YARD".into(), (10, 10)),
            ("REFN".into(), (0, 0)),
            ("REFN".into(), (30, 10)),
            ("PLAIN".into(), (14, 10)),
            ("PLAIN".into(), (18, 10)),
            ("PLAIN".into(), (22, 10)),
        ]
    );
    assert!(!hunting(&sim, owner));
}

/// With a refinery and a war factory standing, the house rebuilds its
/// harvester: the Unit choice in production mode 2.
#[test]
fn a_house_with_a_refinery_and_a_war_factory_rebuilds_its_harvester() {
    let (mut sim, rules, owner) = fixture(500);
    let ids = plan(
        &mut sim,
        &rules,
        owner,
        &[
            ("YARD", (10, 10)),
            ("REFN", (14, 10)),
            ("WEAP", (18, 10)),
            ("PLAIN", (22, 10)),
            ("PLAIN", (26, 10)),
        ],
    );
    assert_eq!(sim.houses[&owner].tracking().war_factories(), 1);
    assert!(check_build_need(&sim, &rules, owner));

    manage_build_queue(&mut sim, &rules, owner, None, FrameEffects::default());

    // 500 + 600 is not above the harvester's 1400; 500 + 1200 is.
    let sold: Vec<bool> = ids.iter().map(|&id| selling(&sim, id)).collect();
    assert_eq!(sold, [false, false, false, true, true]);
    let house = &sim.houses[&owner];
    assert_eq!(
        house.ai_unit_choices().get(UnitChoiceKind::Unit),
        rules
            .type_array_index(ObjectCategory::Vehicle, "HARV")
            .unwrap()
    );
    assert_eq!(house.ai_production().mode(), 2);
    assert_eq!(house.ai_production().building_choice(), -1);
    assert_eq!(house.base_plan().nodes().len(), 5, "no node is planned");
    assert!(!hunting(&sim, owner));
}

/// A house that cannot pay even after selling every node keeps its
/// production and sends everything to hunt.
#[test]
fn a_house_that_cannot_pay_keeps_its_production_and_hunts() {
    let (mut sim, rules, owner) = fixture(100);
    let yard = spawn(&mut sim, &rules, "YARD", "AIHouse", (10, 10));
    let ids = plan(&mut sim, &rules, owner, &[("PLAIN", (14, 10))]);
    yard_builds(&mut sim, &rules, owner, yard, "PLAIN");

    manage_build_queue(&mut sim, &rules, owner, None, FrameEffects::default());

    assert!(selling(&sim, ids[0]));
    assert!(sim.production.factories().building_factory(yard).is_some());
    assert_eq!(
        sim.houses[&owner].ai_production().building_choice(),
        rules.building_type_index("PLAIN").unwrap()
    );
    assert!(hunting(&sim, owner));
}

/// A house that pays only by selling its first node still plans the
/// refinery (no node is open, so at 1), then sends everything to hunt. A
/// sold `Gate=` node and an unsellable building's node leave the plan.
#[test]
fn paying_at_the_first_node_rebuilds_then_hunts() {
    let (mut sim, rules, owner) = fixture(0);
    let ids = plan(
        &mut sim,
        &rules,
        owner,
        &[("YARD", (10, 10)), ("GATE", (14, 10)), ("BUNKER", (18, 10))],
    );

    manage_build_queue(&mut sim, &rules, owner, None, FrameEffects::default());

    // The bunker's node goes unsold; the gate's 300 and the yard's 3000
    // pass 2000 at the first node.
    let sold: Vec<bool> = ids.iter().map(|&id| selling(&sim, id)).collect();
    assert_eq!(sold, [true, true, false]);
    assert_eq!(
        plan_types(&sim, &rules, owner),
        [("YARD".into(), (10, 10)), ("REFN".into(), (0, 0))]
    );
    let house = &sim.houses[&owner];
    assert_eq!(
        house.ai_production().building_choice(),
        rules.building_type_index("REFN").unwrap()
    );
    assert!(hunting(&sim, owner));
}

/// A house whose first open node is its first, the lost yard's, abandons its
/// production and clears its choices once the sale pays, but plans and
/// chooses nothing: everything is sent to hunt (`0x004FE143`).
#[test]
fn a_house_whose_yard_node_is_open_rebuilds_nothing_and_hunts() {
    let (mut sim, rules, owner) = fixture(1800);
    let ids = plan(
        &mut sim,
        &rules,
        owner,
        &[("PLAIN", (14, 10)), ("PLAIN", (18, 10))],
    );
    let lost_yard = BasePlanNode {
        filled: false,
        ..node(&rules, "YARD", (10, 10))
    };
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .base_plan_mut()
        .nodes_mut_for_test()
        .insert(0, lost_yard);
    // A war factory off the plan builds a harvester.
    let factory = spawn(&mut sim, &rules, "WEAP", "AIHouse", (30, 10));
    let harvester = rules
        .type_array_index(ObjectCategory::Vehicle, "HARV")
        .unwrap();
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .ai_unit_choices_mut()
        .set(UnitChoiceKind::Unit, harvester);
    crate::sim::production::factory_ai(&mut sim, &rules, factory, None, FrameEffects::default());
    assert!(
        sim.production
            .factories()
            .building_factory(factory)
            .is_some()
    );
    assert!(check_build_need(&sim, &rules, owner));

    manage_build_queue(&mut sim, &rules, owner, None, FrameEffects::default());

    // 1800 and the last plain's 600 pass the refinery's 2000.
    let sold: Vec<bool> = ids.iter().map(|&id| selling(&sim, id)).collect();
    assert_eq!(sold, [false, true]);
    assert!(
        sim.production
            .factories()
            .building_factory(factory)
            .is_none()
    );
    assert_eq!(
        plan_types(&sim, &rules, owner),
        [
            ("YARD".into(), (10, 10)),
            ("PLAIN".into(), (14, 10)),
            ("PLAIN".into(), (18, 10))
        ]
    );
    let house = &sim.houses[&owner];
    assert_eq!(house.ai_unit_choices().get(UnitChoiceKind::Unit), -1);
    assert_eq!(house.ai_production().building_choice(), -1);
    assert_eq!(house.ai_production().mode(), 0);
    assert!(hunting(&sim, owner));
}

/// A sale that empties the plan leaves the last node's bytes in slot 0; the
/// refinery native then writes at slot 1, past the count, and the count
/// takes in slot 0 instead: the plan holds the sold gate again.
#[test]
fn a_sale_that_empties_the_plan_gets_its_last_node_back() {
    let (mut sim, rules, owner) = fixture(1800);
    spawn(&mut sim, &rules, "YARD", "AIHouse", (10, 10));
    let ids = plan(&mut sim, &rules, owner, &[("GATE", (14, 10))]);

    manage_build_queue(&mut sim, &rules, owner, None, FrameEffects::default());

    assert!(selling(&sim, ids[0]));
    assert_eq!(plan_types(&sim, &rules, owner), [("GATE".into(), (14, 10))]);
    let house = &sim.houses[&owner];
    // The gate still stands, so its node is not open and nothing is chosen.
    assert_eq!(house.ai_production().building_choice(), -1);
    assert_eq!(house.ai_production().mode(), 1);
    assert!(hunting(&sim, owner));
}

/// The strategy tick asks for the need first and acts after the missing
/// factory's sell-off: every building is then selling, so the rebuild drops
/// their nodes unsold and, unable to pay, hunts.
#[test]
fn the_strategy_rebuilds_after_the_missing_factorys_sell_off() {
    let (mut sim, rules, owner) = fixture(0);
    let ids = plan(
        &mut sim,
        &rules,
        owner,
        &[("PLAIN", (14, 10)), ("PLAIN", (18, 10))],
    );
    assert!(check_build_need(&sim, &rules, owner));
    // Past the emergency block's 900 frames after the (constructor's) last
    // attack at frame 0, which would hold the house in state three.
    sim.session.set_binary_frame_for_test(1000);

    crate::sim::house_strategy::update_strategy(
        &mut sim,
        &rules,
        owner,
        None,
        FrameEffects::default(),
    );

    assert!(ids.iter().all(|&id| selling(&sim, id)));
    assert!(sim.houses[&owner].base_plan().nodes().is_empty());
    assert!(hunting(&sim, owner));
}

/// Every retail skirmish country has the economy types the bodies read
/// unchecked (the module's null-dereference residual is dormant). Only
/// Yuri's has no harvester (`HarvesterUnit=HARV,CMIN`) and takes the arms
/// without one.
#[test]
fn retail_countries_have_their_economy_types() {
    let Some(rules) = crate::rules::retail_ini_fixture::retail_rulesmd_rules() else {
        return;
    };
    let countries = [
        "Americans",
        "Alliance",
        "French",
        "Germans",
        "British",
        "Africans",
        "Arabs",
        "Confederation",
        "Russians",
        "YuriCountry",
    ];
    for country in countries {
        let side = rules.country_side_index_raw(country).unwrap();
        let bit = house_country_bit(&rules, country);
        for super_weapons in [false, true] {
            for list in [&rules.build_refinery_types, &rules.build_weapons_types] {
                assert!(
                    first_buildable_from_array(&rules, list, country, side, super_weapons)
                        .is_some(),
                    "{country}: {list:?}"
                );
            }
        }
        assert_eq!(
            first_owner_compatible_harvester(&rules, bit).is_some(),
            country != "YuriCountry",
            "{country}"
        );
        assert!(
            first_owner_compatible(
                &rules,
                &rules.general.base_unit_types,
                ObjectCategory::Vehicle,
                bit
            )
            .is_some(),
            "{country}"
        );
    }
    assert!(rules.build_refinery_types.len() >= 2);
}
