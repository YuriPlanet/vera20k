//! Stock type values in an authored skirmish: CreateBuilding supplies distinct
//! plants, then ordinary paid production retains its original quote. The clear
//! map and completed powered base are admitted priors, not a played opening or
//! an assertion that an unmodified retail map grants twenty-six plants.

use super::*;
use crate::map::entities::EntityCategory;
use crate::sim::house_state::HouseState;
use crate::sim::intern::InternedId;
use crate::sim::runtime::{SimResources, SimRuntime};
use crate::sim::snapshot::GameSnapshot;
use crate::sim::world::{FrameEffects, Simulation, TickLane};
use std::collections::BTreeSet;

const OWNER: &str = "Americans";

fn scene() -> Option<(SimRuntime, InternedId)> {
    let mut rules =
        crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")?.rules;
    let (_, mut assets) = crate::rules::retail_ini_fixture::retail_assets()?;
    crate::map::theater::load_theater(&mut assets, "TEMPERATE");
    rules.bind_building_buildup_assets(&assets, "TEMPERATE");
    assert!(rules.has_buildup("GAREFN"));
    let plant = rules.object("NAINDP").unwrap();
    assert!(plant.factory_plant && !plant.capturable);
    assert_eq!(plant.build_limit, 1);
    assert_eq!(plant.cost_bonuses[1].bits(), 0.75_f32.to_bits());
    assert_eq!(rules.type_cost(rules.object("GAREFN").unwrap()), 600);

    let mut sim = Simulation::with_seed(0x4c9d_ed00);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    super::tests::install_infantry_delivery_fixture_map(&mut sim);
    let owner = sim.interner.intern(OWNER);
    for (name, human) in [(OWNER, true), ("Russians", false)] {
        let id = sim.interner.intern(name);
        let mut house = HouseState::new(id, u8::from(!human), Some(id), human, 50_000, 10);
        house.project_country_mults(&rules, &sim.interner);
        sim.houses.register(house);
    }
    sim.session.set_current_house_for_test(Some(owner));
    sim.session.game_options_mut_for_test().crates = false;
    // Finished base/power and a surviving rival are explicit fixture priors.
    // No plant, production price, Factory, payment or completion is supplied.
    for (kind, house, x, y) in [
        ("GACNST", OWNER, 8, 8),
        ("GAPOWR", OWNER, 14, 8),
        ("NANRCT", OWNER, 18, 8),
        ("NANRCT", OWNER, 24, 8),
        ("NANRCT", OWNER, 30, 8),
        ("NACNST", "Russians", 5, 50),
    ] {
        sim.spawn_object(kind, house, x, y, 0, &rules)
            .expect("the admitted base occupies clear nonoverlapping cells");
    }
    assert!(sim.rebuild_dynamic_navigation(&rules));
    let mut runtime = SimRuntime {
        simulation: sim,
        resources: SimResources::new(rules),
    };
    frame(&mut runtime, owner);
    frame(&mut runtime, owner);
    Some((runtime, owner))
}

fn frame(runtime: &mut SimRuntime, owner: InternedId) {
    let visits = runtime
        .simulation
        .production
        .factories()
        .strip_visits_for_test(owner);
    runtime
        .advance_frame(
            &[],
            67,
            TickLane::Ordinary,
            FrameEffects::default().with_production_strip(&visits, &|_| {}),
        )
        .expect("ordinary bound-resource production frame");
}

fn create_distinct_plants(runtime: &mut SimRuntime, owner: InternedId) {
    // The header is only the trigger-definition parser envelope; the declared
    // 64x64 clear map above remains the placement prior. Actions use actual
    // parsed waypoints and the production CreateBuilding/Unlimbo owners.
    let mut text = String::from(
        "[Map]\nTheater=TEMPERATE\nSize=0,0,2,1\nLocalSize=0,0,2,1\n\
         [IsoMapPack5]\n1=DwALABwBAAIA/////wAAABEAAA==\n\
         [Basic]\nNewINIFormat=4\n\
         [Triggers]\nPLANTS0=Americans,<none>,Plants0,0,1,1,1,0\n\
         PLANTS1=Americans,<none>,Plants1,0,1,1,1,0\n\
         [Events]\nPLANTS0=1,8,0,0\nPLANTS1=1,8,0,0\n\
         [Tags]\nPLANTS0_TAG=0,Plants0,PLANTS0\n\
         PLANTS1_TAG=0,Plants1,PLANTS1\n[Waypoints]\n",
    );
    for i in 0..26 {
        let (x, y) = (30 + (i % 6) * 5, 25 + (i / 6) * 5);
        text.push_str(&format!("{i}={}\n", y * 1000 + x));
    }
    // TriggerType727516 reads each action list into512 bytes. Two short
    // lists preserve all26 native action operands without truncation.
    text.push_str("[Actions]\n");
    for group in 0..2 {
        text.push_str(&format!("PLANTS{group}=13"));
        for index in (group * 13)..((group + 1) * 13) {
            let letter = b'A' + index;
            text.push_str(&format!(",125,10,NAINDP,0,0,0,0,{}", char::from(letter)));
        }
        text.push('\n');
    }
    let actions =
        crate::map::actions::parse_actions(&crate::rules::ini_parser::IniFile::from_str(&text));
    assert_eq!(actions.len(), 2);
    assert!(actions.values().all(|list| list.entries().len() == 13));
    let map = crate::map::map_file::MapFile::from_bytes(text.as_bytes()).unwrap();
    let sim = &mut runtime.simulation;
    sim.session.install_waypoints(&map.waypoints);
    sim.initialize_trigger_definitions(&map, &Default::default(), None, None);
    sim.classify_map_triggers(&runtime.resources.rules);
    frame(runtime, owner);
    // Removing the first one-shot Tag compacts the forward live walk. Let the
    // second ordinary visit reach the other Tag; no production state is forced.
    frame(runtime, owner);
    let sim = &runtime.simulation;
    let plants: Vec<_> = sim
        .entities()
        .iter_sorted()
        .filter_map(|(id, entity)| {
            (entity.owner() == owner && sim.resolve(entity.type_ref()) == "NAINDP").then(|| {
                assert!(entity.lifecycle.object_alive() && !entity.lifecycle.in_limbo());
                assert!(entity.lifecycle.cell_marked());
                id
            })
        })
        .collect();
    assert_eq!(
        plants.len(),
        26,
        "every parsed action constructs a real plant"
    );
    assert_eq!(plants.iter().copied().collect::<BTreeSet<_>>().len(), 26);
    assert_eq!(
        sim.houses[&owner].base_projection().factory_plants(),
        plants
    );
    assert_eq!(
        sim.cost_of(
            owner,
            runtime.resources.rules.object("GAREFN").unwrap(),
            &runtime.resources.rules
        ),
        600
    );
}

fn begin_refinery(runtime: &mut SimRuntime, owner: InternedId) -> u64 {
    let type_id = runtime.simulation.interner.get("GAREFN").unwrap();
    assert!(
        build_options_for_owner(&runtime.simulation, &runtime.resources.rules, OWNER)
            .iter()
            .any(|option| option.type_id == type_id && option.enabled),
        "the admitted stock base meets the refinery's actual build prerequisites"
    );
    assert!(enqueue_by_type(
        &mut runtime.simulation,
        &runtime.resources.rules,
        OWNER,
        "GAREFN",
        FrameEffects::default(),
        false,
    ));
    runtime
        .simulation
        .production
        .factories()
        .view(owner, ProductionCategory::Building)
        .unwrap()
        .object
        .unwrap()
        .entity_id
        .unwrap()
}

fn finish_history(start_before_plants: bool) {
    let Some((mut runtime, owner)) = scene() else {
        return;
    };
    let early = start_before_plants.then(|| {
        let id = begin_refinery(&mut runtime, owner);
        assert_eq!(
            runtime
                .simulation
                .entities()
                .get(id)
                .unwrap()
                .production_start_price(),
            2000
        );
        let sim = &mut runtime.simulation;
        assert!(suspend_production(
            &sim.interner,
            &mut sim.production,
            &sim.session,
            OWNER,
            ProductionCategory::Building
        ));
        id
    });
    create_distinct_plants(&mut runtime, owner);
    let product = early.unwrap_or_else(|| begin_refinery(&mut runtime, owner));
    let price = if start_before_plants { 2000 } else { 600 };
    assert_eq!(
        runtime
            .simulation
            .entities()
            .get(product)
            .unwrap()
            .production_start_price(),
        price
    );
    if start_before_plants {
        // Current-layout snapshot retention is checked without claiming original
        // SAV compatibility or replacing the runtime's map/resource admission.
        let saved = GameSnapshot::save(&runtime.simulation, 0, 0, "start quote", 0);
        let loaded = GameSnapshot::load(&saved).unwrap().sim;
        assert_eq!(
            loaded
                .entities()
                .get(product)
                .unwrap()
                .production_start_price(),
            price
        );
        assert_eq!(
            begin_refinery(&mut runtime, owner),
            product,
            "same-type resume retains identity"
        );
    }
    for _ in 0..5_000 {
        if ready_building_entity_for_owner(
            &runtime.simulation,
            &runtime.resources.rules,
            owner,
            runtime.simulation.interner.get("GAREFN").unwrap(),
        )
        .is_some()
        {
            break;
        }
        frame(&mut runtime, owner);
    }
    let sim = &runtime.simulation;
    let factory = sim
        .production
        .factories()
        .view(owner, ProductionCategory::Building)
        .unwrap();
    assert_eq!(
        factory.progress, PRODUCTION_STEPS,
        "ordinary payment must finish"
    );
    assert_eq!(
        sim.production
            .factories()
            .factory(FactoryHolder::House(owner, ProductionCategory::Building))
            .unwrap()
            .balance(),
        0
    );
    assert_eq!(sim.houses[&owner].economy().spent_credits(), price);
    assert!(place_production_with_overlays(
        &mut runtime.simulation,
        &runtime.resources.rules,
        OWNER,
        ProductionPlacement::Building {
            type_id: "GAREFN",
            cell: (12, 13)
        },
        Some(&runtime.resources.overlay_registry),
        FrameEffects::default(),
    ));
    for _ in 0..600 {
        if runtime
            .simulation
            .entities()
            .get(product)
            .unwrap()
            .building_art
            .actually_placed()
        {
            break;
        }
        frame(&mut runtime, owner);
    }
    let sim = &mut runtime.simulation;
    assert!(
        sim.entities()
            .get(product)
            .unwrap()
            .building_art
            .actually_placed(),
        "natural buildup must open the retained refinery"
    );
    assert_eq!(
        sim.entities()
            .get(product)
            .unwrap()
            .production_start_price(),
        price
    );
    let miners = sim
        .entities()
        .iter_sorted()
        .filter(|(_, entity)| {
            entity.owner() == owner
                && entity.category() == EntityCategory::Unit
                && sim.resolve(entity.type_ref()) == "CMIN"
        })
        .count();
    assert_eq!(
        miners,
        usize::from(start_before_plants),
        "FreeUnit uses the retained start quote, not its current600 price"
    );
    let hash = sim.state_hash();
    sim.substrate
        .entities
        .get_mut(product)
        .unwrap()
        .record_production_start_price(price + 1);
    assert_ne!(
        sim.state_hash(),
        hash,
        "released Factory cannot be the quote's hash authority"
    );
    sim.substrate
        .entities
        .get_mut(product)
        .unwrap()
        .record_production_start_price(price);
    assert_eq!(sim.state_hash(), hash);
}

#[test]
fn stock_refinery_started_after_twenty_six_real_plants_suppresses_free_unit() {
    finish_history(false);
}

#[test]
fn stock_refinery_start_quote_survives_later_plants_hold_and_resume() {
    finish_history(true);
}
