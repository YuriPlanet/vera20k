//! Physical ALL05 aircraft production at the authored106 action boundary.
//!
//! Healthy, completed buildings on an admitted flat crop are declared priors;
//! this is not a replay of Event55 or of the whole campaign. Original Factory
//! payment and House/command-tail observations are in the consumed corpus.

use super::*;
use crate::rules::process_owner::NativeScenarioRulesPrefix;
use crate::sim::command::Command;
use crate::sim::intern::InternedId;
use crate::sim::production::{self, CanBuild, ProductionCategory};
use crate::sim::runtime::{SimResources, SimRuntime};
use crate::sim::world::TickLane;

fn production_stage<'a>(history: &'a Value, name: &str) -> &'a Value {
    history["stages"]
        .as_array()
        .expect("the original production history must have executed")
        .iter()
        .find(|stage| stage["name"] == name)
        .unwrap_or_else(|| panic!("missing executed native production boundary {name}"))
}

fn can_build_value(sim: &Simulation, rules: &RuleSet, owner: InternedId, id: &str) -> i32 {
    let object = rules
        .object_in_category(ObjectCategory::Aircraft, id)
        .unwrap();
    match production::can_build(sim, rules, owner, object, false, true) {
        CanBuild::No => 0,
        CanBuild::Yes => 1,
        CanBuild::AtLimit => -1,
    }
}

fn assert_production_state(
    sim: &Simulation,
    rules: &RuleSet,
    owner: InternedId,
    original: &Value,
    observed_frame: u32,
) {
    let name = original["name"].as_str().unwrap();
    assert_eq!(
        i64::from(observed_frame),
        original["frame"].as_i64().unwrap(),
        "{name}"
    );
    for id in ["ORCA", "BEAG"] {
        let object = rules
            .object_in_category(ObjectCategory::Aircraft, id)
            .unwrap();
        assert_eq!(
            sim.object_tech_level(object),
            number(&original["tech_levels"][id]),
            "{name}/{id}: shared current type field"
        );
        assert_eq!(
            can_build_value(sim, rules, owner, id),
            number(&original["can_build"][id]),
            "{name}/{id}: original CanBuild(0,1)"
        );
    }
    let house = &sim.houses[&owner];
    assert_eq!(
        house.tech_recheck_pending(),
        original["dirty"].as_bool().unwrap(),
        "{name}: House+1FC"
    );
    assert_eq!(
        house.economy.credits(),
        number(&original["credits"]),
        "{name}: actual wallet"
    );
    assert_eq!(
        house.economy.spent_credits(),
        number(&original["spent"]),
        "{name}: money-spent statistic"
    );
    let actual = sim
        .production
        .factories
        .iter_insertion_ordered()
        .into_iter()
        .find(|factory| factory.owner == owner && factory.category == ProductionCategory::Aircraft);
    let expected = &original["factory"];
    if expected.is_null() || expected["exists"] == false {
        assert!(actual.is_none(), "{name}: the empty Factory was deleted");
    } else {
        let factory = actual.expect("native retained the aircraft Factory");
        assert_eq!(
            i32::from(factory.progress),
            number(&expected["stage"]),
            "{name}"
        );
        assert_eq!(factory.balance, number(&expected["balance"]), "{name}");
        assert_eq!(
            i32::from(factory.step_rate_frames),
            number(&expected["rate"]),
            "{name}"
        );
        // CdTimer owns the start/duration fields. The unused native middle
        // timer dword is outside this representation and this comparison.
        assert_eq!(
            factory.step_timer.start_frame(),
            number(&expected["timer"][0]),
            "{name}"
        );
        assert_eq!(
            factory.step_timer.duration(),
            number(&expected["timer"][2]),
            "{name}"
        );
        assert_eq!(
            factory
                .object
                .as_ref()
                .map(|object| sim.interner.resolve(object.type_id)),
            expected["held_type"].as_str(),
            "{name}: same held type"
        );
    }
}

fn retail_production_world(history: &Value) -> Option<(SimRuntime, InternedId)> {
    let (_, assets) = crate::rules::retail_ini_fixture::retail_assets()?;
    let input = &history["input"];
    let map_name = input["map"].as_str().unwrap();
    let map_raw = assets.get_ref(map_name).unwrap();
    let recorded_source = corpus()["retail"]["sources"]
        .as_array()
        .unwrap()
        .iter()
        .find(|source| source["file"] == map_name)
        .unwrap();
    assert_eq!(
        crate::util::sha256::sha256_hex(map_raw),
        recorded_source["sha256"]
    );
    let map = crate::map::map_file::MapFile::from_bytes(map_raw).unwrap();
    let route = corpus()["retail"]["routes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|route| route["map"] == map_name)
        .unwrap();
    assert_eq!(
        map.ini.section("Actions").unwrap().read_string(
            route["trigger_id"].as_str().unwrap(),
            "",
            512
        ),
        route["raw_actions"].as_str().unwrap(),
        "the selected authored route is the physical ALL05 action list"
    );
    let authored = crate::map::actions::parse_actions(&map.ini);
    let entries = authored[route["trigger_id"].as_str().unwrap()].entries();
    let declared = input["raw_actions"].as_array().unwrap();
    let changes: Vec<_> = entries.iter().filter(|entry| entry.kind == 106).collect();
    assert_eq!(changes.len(), declared.len());
    for (entry, raw) in changes.iter().zip(declared) {
        let control = crate::map::actions::parse_actions(&IniFile::from_str(&format!(
            "[Actions]\nCONTROL=1,{}\n",
            raw.as_str().unwrap()
        )));
        let original = &control["CONTROL"].entries()[0];
        assert_eq!(entry.inline_type_name(), original.inline_type_name());
        assert_eq!(entry.literal_value(), original.literal_value());
    }
    let named_name = map_name.replace(".MAP", ".INI");
    let named = assets.resolve_ref(&named_name).map(|_| {
        crate::rules::retail_sources::select_ini(&assets, &named_name)
            .unwrap()
            .ini
    });
    let mut rules_owner = crate::rules::retail_ini_fixture::retail_rules_owner(&assets);
    let (mut rules, _, fixed_art, _) = rules_owner
        .load_scenario(
            NativeScenarioRulesPrefix::Campaign(named.as_ref()),
            &map.ini,
        )
        .unwrap()
        .into_parts();
    rules.install_art_data(crate::rules::retail_ini_fixture::art_registry(&fixed_art));
    rules.bind_animation_sequences(
        &crate::rules::infantry_sequence::parse_infantry_sequence_registry(&fixed_art),
    );
    let owner_name = input["house"].as_str().unwrap();
    let country_name = input["country"].as_str().unwrap();
    let authored_house = map.ini.section(owner_name).unwrap();
    assert_eq!(
        authored_house.read_name("Country", 0x80),
        Some(country_name)
    );
    assert_eq!(
        authored_house.read_int("TechLevel", 255),
        number(&input["house_tech"])
    );
    assert_eq!(
        rules.country_index(country_name).unwrap().0,
        input["country_index"].as_u64().unwrap() as u16
    );
    assert_eq!(
        map.ini.section("Basic").unwrap().read_name("Player", 0x80),
        Some(owner_name)
    );

    let mut sim = Simulation::with_seed(0);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let geometry = &input["geometry"];
    let width = u16::try_from(number(&geometry["map_size"][0])).unwrap();
    let height = u16::try_from(number(&geometry["map_size"][1])).unwrap();
    let bounds = &geometry["coordinate_bounds"];
    let extent_x = u16::try_from(number(&bounds[2])).unwrap();
    let extent_y = u16::try_from(number(&bounds[3])).unwrap();
    assert_eq!(extent_x, extent_y, "the declared flat allocation is square");
    assert_eq!(number(&bounds[0]), 0);
    assert_eq!(number(&bounds[1]), 0);
    // Original Map+EC declares16x16; +FC is the admitted0..32 cell crop.
    // Its allocated Class/height planes include the extra last row/column.
    sim.install_resolved_terrain_for_new_map(crate::map::resolved_terrain::test_flat_ground_grid(
        extent_x.checked_add(1).unwrap(),
    ));
    sim.session.map_width = width;
    sim.session.map_height = height;
    sim.playfield_bounds = Some(crate::sim::cell_rect::PlayfieldBounds {
        base: i32::from(width),
        off_fc: number(&bounds[0]),
        off_100: number(&bounds[1]),
        off_104: number(&bounds[2]),
        off_108: number(&bounds[3]),
    });
    sim.playfield_size_height = Some(i32::from(height));
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(false));
    sim.set_input_delay_ticks(0);
    let owner = sim.interner.intern(owner_name);
    let country = sim.interner.intern(country_name);
    let side = rules.country_side_index(country_name).unwrap();
    let mut house = HouseState::new(
        owner,
        side.0,
        Some(country),
        true,
        number(&input["initial_credits"]),
        number(&input["house_tech"]),
    );
    house.project_country_mults(&rules, &sim.interner);
    sim.houses.insert(owner, house);
    sim.session.house_order.push(owner);
    sim.session.current_house = Some(owner);
    for building in geometry["owned_buildings"].as_array().unwrap() {
        sim.spawn_object(
            building["type"].as_str().unwrap(),
            owner_name,
            u16::try_from(number(&building["cell"][0])).unwrap(),
            u16::try_from(number(&building["cell"][1])).unwrap(),
            0,
            &rules,
        )
        .expect("declared healthy completed building is admitted on the flat crop");
    }
    // Establish the declared completed-building power prior through its
    // existing assessment owner, before importing the observed RNG cursors.
    // Native health sampling440042 and House assessment4F84D9 produced these
    // totals; an empty PowerState is not the same Factory input.
    crate::sim::power_system::tick_power_states(
        &mut sim.power_states,
        &mut sim.substrate.entities,
        &rules,
        &sim.interner,
        sim.session.binary_frame,
    );
    let power = &sim.power_states[&owner];
    assert_eq!(
        json!([power.total_output, power.total_drain]),
        input["house_power"]
    );
    assert_eq!(
        sim.houses[&owner].tracking.airport_docks(),
        number(&input["airport_docks"])
    );
    if !history["initial_state"]["dirty"].as_bool().unwrap() {
        assert!(sim.houses.get_mut(&owner).unwrap().take_tech_recheck());
    }
    sim.session.binary_frame = u32::try_from(number(&input["initial_frame"])).unwrap();
    // These are complete observed Random2 objects at the production boundary,
    // after original setup. Importing them does not assert constructor parity.
    let random = &history["rng_seed_states"];
    sim.scenario_rng = SimRng::from_native_state_hex_for_test(random["scenario"].as_str().unwrap());
    sim.main_rng = SimRng::from_native_state_hex_for_test(random["main"].as_str().unwrap()).into();
    sim.mapgen_rng = SimRng::from_native_state_hex_for_test(random["mapgen"].as_str().unwrap());
    let mut resources = SimResources::empty();
    resources.rules = rules;
    Some((
        SimRuntime {
            simulation: sim,
            resources,
        },
        owner,
    ))
}

#[test]
fn retail_all05_type_tech_level_recheck_cancels_paid_harrier_and_unlocks_black_eagle() {
    let history = &corpus()["production_history"];
    let Some((mut runtime, owner)) = retail_production_world(history) else {
        return;
    };
    let input = &history["input"];
    assert_production_state(
        &runtime.simulation,
        &runtime.resources.rules,
        owner,
        &history["initial_state"],
        runtime.simulation.session.binary_frame,
    );
    assert_eq!(
        rng_hashes(&runtime.simulation),
        history["initial_state"]["rng"],
        "complete native Random2 objects are the declared production prior"
    );
    assert!(production::enqueue_by_type(
        &mut runtime.simulation,
        &runtime.resources.rules,
        input["house"].as_str().unwrap(),
        "ORCA",
        FrameEffects::default(),
        false,
    ));
    let held = runtime
        .simulation
        .production
        .factories
        .view(owner, ProductionCategory::Aircraft)
        .unwrap()
        .object
        .unwrap()
        .entity_id
        .unwrap();
    assert!(
        runtime
            .simulation
            .entities()
            .get(held)
            .unwrap()
            .lifecycle
            .in_limbo()
    );
    let paid = production_stage(history, "paid_orca");
    let paid_frame = u32::try_from(number(&paid["frame"])).unwrap();
    assert!(paid_frame >= runtime.simulation.session.binary_frame);
    while runtime.simulation.session.binary_frame < paid_frame {
        runtime.simulation.session.binary_frame += 1;
        production::step_factories(&mut runtime.simulation);
    }
    assert_production_state(
        &runtime.simulation,
        &runtime.resources.rules,
        owner,
        paid,
        runtime.simulation.session.binary_frame,
    );
    assert_eq!(rng_hashes(&runtime.simulation), paid["rng"]);
    let actions = input["raw_actions"].as_array().unwrap();
    assert_eq!(actions.len(), 2);
    for (raw, name) in actions.iter().zip(["action_orca", "action_beag"]) {
        assert!(apply(
            &mut runtime.simulation,
            &runtime.resources.rules,
            raw.as_str().unwrap()
        ));
        let original = production_stage(history, name);
        assert_production_state(
            &runtime.simulation,
            &runtime.resources.rules,
            owner,
            original,
            runtime.simulation.session.binary_frame,
        );
        assert_eq!(rng_hashes(&runtime.simulation), original["rng"]);
    }

    // This bound production frame reaches the existing private House owner,
    // its generated ABANDON events and their same-frame command dispatcher.
    // The native receipt stops before MainTick's frame increment. Rust's API
    // additionally commits that frame; observe the tail's pre-commit frame.
    let native_tail_frame = runtime.simulation.session.binary_frame;
    let mut output = runtime
        .advance_frame(&[], 67, TickLane::Ordinary, FrameEffects::default())
        .unwrap();
    let commands = output.take_admitted_commands();
    let events: Vec<_> = commands
        .iter()
        .map(|command| {
            assert_eq!(command.owner, owner);
            assert_eq!(command.execute_tick, output.tick.tick);
            let Command::CancelProductionByType { type_id, all } = &command.payload else {
                panic!(
                    "unexpected generated production event {:?}",
                    command.payload
                )
            };
            let id = runtime.simulation.interner.resolve(*type_id);
            let object = runtime.resources.rules.object(id).unwrap();
            assert_eq!(object.category, ObjectCategory::Aircraft);
            // Native's declared fixture constructs selected AircraftTypes;
            // Rust reads the complete retail registry. Compare the event's
            // logical type via the original observed subtype-index mapping,
            // rather than equating two different registry allocation histories.
            let native_type = input["native_aircraft_registry"]
                .as_array()
                .unwrap()
                .iter()
                .find(|entry| entry["id"] == id)
                .expect("the generated command names an observed native AircraftType");
            json!({
                "kind": if *all { 0x2e } else { 0x10 },
                "frame": native_tail_frame,
                "rtti": 3,
                "index": native_type["index"],
            })
        })
        .collect();
    assert_eq!(
        json!(events),
        production_stage(history, "house_recheck")["events"]
    );
    assert_production_state(
        &runtime.simulation,
        &runtime.resources.rules,
        owner,
        production_stage(history, "receive"),
        native_tail_frame,
    );
    assert_eq!(
        runtime.simulation.session.binary_frame,
        native_tail_frame.wrapping_add(1)
    );
    assert!(
        !runtime.simulation.entities().contains(held),
        "command tail destroys the paid held Aircraft"
    );
    assert!(!runtime.simulation.logic_order().contains(&held));
    production::validate_restored_factory_state(&runtime.simulation, &runtime.resources.rules)
        .unwrap();
}
