//! Authored campaign wall/gate planning through House production choices.

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::rules::process_owner::NativeScenarioRulesPrefix;
use crate::sim::campaign_carry_over::CampaignCarryOver;
use crate::sim::world::TickLane;

fn campaign_perimeter(
    expansions: usize,
    overrides: &str,
) -> Option<(Simulation, RuleSet, InternedId)> {
    let (_, assets) = crate::rules::retail_ini_fixture::retail_assets()?;
    let mut source = format!(
        "[Basic]\nPlayer=Player\n\
         [Houses]\n0=WallBase\n1=Player\n\
         [WallBase]\nCountry=Americans\nIQ=5\nCredits=200\nNodeCount={}\n",
        expansions + 1,
    );
    for index in 0..expansions {
        source.push_str(&format!("{index:03}=-3,0,0\n"));
    }
    source.push_str(&format!(
        "{expansions:03}=GACNST,12,12\n[Player]\nCountry=Russians\nPlayerControl=yes\n{overrides}"
    ));
    let ini = IniFile::from_str(&source);
    let (rules, _, _, _) = crate::rules::retail_ini_fixture::retail_rules_owner(&assets)
        .load_scenario(NativeScenarioRulesPrefix::Campaign(None), &ini)
        .unwrap()
        .into_parts();
    let roster = crate::map::houses::parse_house_roster(&ini, &rules.color_schemes, Some(&rules));
    let mut sim = Simulation::with_seed(0x5082C0);
    crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
    sim.overlay_grid = Some(crate::sim::overlay_grid::OverlayGrid::new(32, 32));
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.session
        .initialize_campaign_startup(1, 1, None, 0, CampaignCarryOver::CLEARED)
        .unwrap();
    crate::sim::scenario_bootstrap::initialize_map_roster_houses(
        &mut sim,
        &roster,
        &rules,
        Some(&ini),
    );
    crate::sim::scenario_bootstrap::initialize_campaign_current_house(
        &mut sim.session,
        &mut sim.houses,
        &ini,
    );
    let owner = sim.interner.get("WallBase").unwrap();
    sim.spawn_object("GACNST", "WallBase", 12, 12, 0, &rules)
        .unwrap();
    assert!(sim.trigger_auto_base_building(
        owner,
        1,
        &rules,
        Some(rules.overlay_registry()),
        FrameEffects::default(),
    ));
    sim.session.publish_started();
    Some((sim, rules, owner))
}

fn frame(sim: &mut Simulation, rules: &RuleSet) {
    sim.advance_app_frame(
        &[],
        rules,
        Some(rules.overlay_registry()),
        67,
        TickLane::Ordinary,
        FrameEffects::default(),
    )
    .unwrap();
}

/// Original ChooseBuilding4FE4B0..4FE518 removes -3, expands through5082C0,
/// and returns before selecting a generated wall on a later House visit.
#[test]
fn authored_campaign_perimeter_expands_through_the_ordinary_house_rung() {
    let Some((mut sim, rules, owner)) = campaign_perimeter(1, "") else {
        return;
    };
    assert_eq!(rules.ns_gate_types, ["GADUMY"]);
    assert_eq!(rules.ew_gate_types, ["GADUMY"]);
    assert_eq!(rules.ai_base_spacing, 1);
    assert_eq!(
        rules.object("GACNST").unwrap().foundation.dimensions(),
        (4, 4)
    );
    assert_eq!(
        sim.houses[&owner].base_plan().nodes()[0].type_or_control,
        -3
    );
    let primary_perimeter = sim.houses[&owner]
        .base_reservation()
        .perimeter_cells()
        .to_vec();
    assert_eq!(
        sim.houses[&owner].base_reservation().bounds(),
        (11, 11, 6, 6)
    );
    frame(&mut sim, &rules);
    let house = &sim.houses[&owner];
    assert_eq!(house.ai_production().building_choice(), -1);
    assert_eq!(
        house.base_plan().nodes()[0].type_or_control,
        rules.building_type_index("GACNST").unwrap()
    );
    assert!(
        house.base_plan().nodes().len() > 1,
        "the authored -3 must append the native wall/gate plan"
    );
    assert_eq!(
        house.base_reservation().perimeter_cells(),
        primary_perimeter
    );
    assert_eq!(house.base_reservation().bounds(), (10, 10, 8, 8));
    let nodes = house.base_plan().nodes();
    assert_eq!(nodes.len(), 21);
    let wall = rules.building_type_index("GAWALL").unwrap();
    assert!(
        nodes[1..17]
            .iter()
            .all(|node| node.type_or_control == wall && !node.filled && node.retry_count == 0)
    );
    assert_eq!(unpack_base_plan_cell(nodes[1].packed_cell), (10, 10));
    let gate = rules.building_type_index("GADUMY").unwrap();
    assert!(nodes[17..].iter().all(|node| node.type_or_control == gate));
    assert_eq!(
        nodes[17..]
            .iter()
            .map(|node| unpack_base_plan_cell(node.packed_cell))
            .collect::<Vec<_>>(),
        [(13, 10), (13, 17), (10, 13), (17, 13)]
    );
    for _ in 0..8 {
        frame(&mut sim, &rules);
    }
    assert_eq!(sim.houses[&owner].ai_production().building_choice(), wall);
}

///508672..5088D8 distinguishes soft overlay/slope breaks from hard terrain
/// breaks; long trailing runs permit gates even after earlier soft breaks.
#[test]
fn perimeter_breaks_preserve_short_walls_and_gate_orientation_without_rng() {
    let Some((mut sim, rules, owner)) =
        campaign_perimeter(1, "[AI]\nNSGates=GAPILL\nEWGates=GAPOWR\n")
    else {
        return;
    };
    assert_eq!(rules.ns_gate_types, ["GAPILL"]);
    assert_eq!(rules.ew_gate_types, ["GAPOWR"]);
    sim.resolved_terrain
        .as_mut()
        .unwrap()
        .cell_mut_for_test(14, 10)
        .unwrap()
        .slope_type = 1;
    sim.resolved_terrain
        .as_mut()
        .unwrap()
        .cell_mut_for_test(17, 10)
        .unwrap()
        .land_type = crate::rules::terrain_rules::LandType::Rock.as_index();
    // A short clear run before a hard break is discarded. The remainder of
    // the left edge has length4; it is also too short for a trailing gate.
    sim.resolved_terrain
        .as_mut()
        .unwrap()
        .cell_mut_for_test(10, 12)
        .unwrap()
        .land_type = crate::rules::terrain_rules::LandType::Water.as_index();
    let rng = sim.scenario_rng.logical_state();
    // Building choice is the real 4FE3E0 caller. Its sibling unit choices
    // retain their own RNG draws, outside the perimeter helper's contract.
    choose_building(&mut sim, &rules, owner, Some(rules.overlay_registry()));
    assert_eq!(sim.scenario_rng.logical_state(), rng);
    let nodes = sim.houses[&owner].base_plan().nodes();
    let walls: Vec<_> = nodes
        .iter()
        .filter(|node| {
            rules
                .building_type_at(node.type_or_control)
                .is_some_and(|ty| ty.id == "GAWALL")
        })
        .map(|node| unpack_base_plan_cell(node.packed_cell))
        .collect();
    assert!(
        walls.contains(&(10, 10)) && walls.contains(&(13, 10)),
        "soft break flushes the four preceding walls"
    );
    assert!(
        walls.contains(&(15, 10)) && walls.contains(&(16, 10)),
        "a soft break suppresses the next hard-break gate, retaining its short run"
    );
    assert!(
        !walls.contains(&(10, 11)) && !walls.contains(&(10, 13)),
        "hard-ended and trailing short runs are discarded"
    );
    let gates: Vec<_> = nodes
        .iter()
        .filter_map(|node| {
            let ty = rules.building_type_at(node.type_or_control)?;
            matches!(ty.id.as_str(), "GAPOWR" | "GAPILL")
                .then(|| (ty.id.as_str(), unpack_base_plan_cell(node.packed_cell)))
        })
        .collect();
    assert_eq!(gates, [("GAPOWR", (13, 17)), ("GAPILL", (17, 13))]);
}

/// BaseSave42F070/Load42EE30 retain the committed bounds. A second scan
/// grows those loaded bounds; ordinary FactoryAI4500F0 places its first wall.
#[test]
fn loaded_perimeter_expands_again_and_the_factory_places_a_generated_wall() {
    use crate::sim::snapshot::GameSnapshot;

    let Some((mut sim, rules, owner)) = campaign_perimeter(2, "") else {
        return;
    };
    frame(&mut sim, &rules);
    assert_eq!(
        sim.houses[&owner].base_plan().nodes()[0].type_or_control,
        -3
    );
    assert_eq!(
        sim.houses[&owner].base_reservation().bounds(),
        (10, 10, 8, 8)
    );
    let bytes = GameSnapshot::save_validated(&sim, 0, 0, "perimeter_arena", 0);
    let mut sim = GameSnapshot::load_validated(&bytes, 0, 0, sim.session.map_name())
        .unwrap()
        .sim;
    sim.restore_after_snapshot_load().unwrap();
    crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
    for _ in 0..8 {
        frame(&mut sim, &rules);
    }
    assert_eq!(
        sim.houses[&owner].base_reservation().bounds(),
        (9, 9, 10, 10)
    );
    assert!(
        sim.houses[&owner]
            .base_plan()
            .nodes()
            .iter()
            .all(|node| node.type_or_control != -3)
    );
    // The same action3 owner used by authored campaign startup; no finished
    // Factory or product is inserted by this fixture.
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .enable_scripted_production();
    let overlay = rules.overlay_registry().id_for_name("GAWALL").unwrap();
    for _ in 0..2000 {
        frame(&mut sim, &rules);
        if sim.overlay_grid.as_ref().unwrap().cell(10, 10).overlay_id == Some(overlay) {
            assert_eq!(
                sim.overlay_grid.as_ref().unwrap().cell(10, 10).wall_owner,
                Some(owner)
            );
            return;
        }
    }
    panic!("ordinary campaign FactoryAI must place its generated GAWALL");
}
