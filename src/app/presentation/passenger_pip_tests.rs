//! Stock passenger boarding/departure through the production cargo and display
//! owners. The drawing oracle separately executes the original pip consumer.
use crate::rules::ruleset::RuleSet;
use crate::sim::command::Command;
use crate::sim::world::Simulation;

const OWNER: &str = "Americans";

fn frames(sim: &Simulation, rules: &RuleSet, transport: u64, hover: Option<u64>) -> Vec<usize> {
    let hash = sim.state_hash();
    let result = super::ui_overlays::scale_pip_draws(
        sim,
        sim.entities().get(transport).unwrap(),
        hover,
        Some(OWNER),
        rules,
    )
    .into_iter()
    .map(|pip| pip.frame)
    .collect();
    assert_eq!(
        sim.state_hash(),
        hash,
        "status queries cannot change retained simulation state"
    );
    result
}

pub(super) fn apply_native_size_override_for_test(
    rules: &mut RuleSet,
    size_override: &serde_json::Value,
) {
    let type_name = size_override["type_name"].as_str().unwrap();
    let bits = u64::from_str_radix(size_override["f64_bits"].as_str().unwrap(), 16).unwrap();
    rules.set_object_size_for_test(type_name, f64::from_bits(bits));
    assert_eq!(rules.object(type_name).unwrap().size.to_bits(), bits);
}

#[test]
fn stock_passenger_pips_follow_real_boarding_departure_and_selection() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    let rules = &retail.rules;
    let native = crate::test_fixture::json("tools/procedural_drawing_oracle/passenger_pips.json");
    for (transport_type, passenger_type, control, total_size) in [
        ("FV", "E1", "loaded_a127", 1),
        ("FV", "ENGINEER", "engineer", 1),
        ("LCRF", "MTNK", "integer_size_passenger", 3),
    ] {
        let row = native["raster_controls"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["name"] == control)
            .unwrap();
        let loaded: Vec<_> = row["draws"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|draw| draw["asset"] == "PIPS2.SHP")
            .map(|draw| draw["frame"].as_u64().unwrap() as usize)
            .collect();
        //709D38 zero-initializes GetPipMax slots; loaded frames above are
        //actual native executions, including the Size>1 own/filler ordering.
        let empty = vec![0; rules.object(transport_type).unwrap().pip_max(rules) as usize];
        let mut sim = Simulation::new();
        let owner = sim.intern(OWNER);
        sim.houses_mut_for_test()
            .register(crate::sim::house_state::HouseState::new(
                owner, 0, None, true, 0, 10,
            ));
        crate::sim::arena_fixture::flat_arena(&mut sim, rules);
        sim.intern_rule_type_ids(rules);
        sim.resolve_type_handles(rules);
        sim.fog_mut_for_test().reveal_all_for_owner(owner);
        let transport = sim
            .spawn_object(transport_type, OWNER, 16, 16, 64, rules)
            .unwrap();
        let passenger = sim
            .spawn_object(passenger_type, OWNER, 16, 15, 64, rules)
            .unwrap();
        assert!(sim.apply_command(
            OWNER,
            &Command::Select {
                entity_ids: vec![transport],
                additive: false,
            },
            Some(rules)
        ));
        assert_eq!(frames(&sim, rules, transport, None), empty);
        assert!(sim.apply_command(
            OWNER,
            &Command::clicked_mission(
                passenger,
                crate::sim::mission::MissionType::Enter,
                crate::sim::combat::TargetKind::Entity(transport)
            ),
            Some(rules)
        ));
        let grid = sim.path_grid_snapshot().unwrap();
        for _ in 0..240 {
            sim.advance_tick(&[], Some(rules), Some(&grid), None, 33);
            if sim
                .entities()
                .get(passenger)
                .unwrap()
                .passenger_role
                .is_inside_transport()
            {
                break;
            }
        }
        let rider = sim.entities().get(passenger).unwrap();
        assert_eq!(
            rider.passenger_role.inside_transport(),
            Some((transport, false))
        );
        assert!(rider.lifecycle.in_limbo());
        let hold = sim
            .entities()
            .get(transport)
            .unwrap()
            .passenger_role
            .cargo()
            .unwrap();
        assert_eq!(hold.passengers(), [passenger]);
        assert_eq!(hold.total_size(), total_size);
        assert_eq!(frames(&sim, rules, transport, None), loaded, "{control}");

        assert!(sim.apply_command(
            OWNER,
            &Command::UnloadPassengers {
                transport_id: transport
            },
            Some(rules)
        ));
        for _ in 0..240 {
            sim.advance_tick(&[], Some(rules), Some(&grid), None, 33);
            if !sim
                .entities()
                .get(passenger)
                .unwrap()
                .passenger_role
                .is_inside_transport()
            {
                break;
            }
        }
        let rider = sim.entities().get(passenger).unwrap();
        assert!(!rider.passenger_role.is_inside_transport());
        assert!(!rider.lifecycle.in_limbo());
        assert_eq!(
            sim.entities()
                .get(transport)
                .unwrap()
                .passenger_role
                .passenger_count(),
            0
        );
        assert_eq!(frames(&sim, rules, transport, None), empty);
        assert!(sim.apply_command(
            OWNER,
            &Command::Select {
                entity_ids: vec![],
                additive: false
            },
            Some(rules)
        ));
        assert!(frames(&sim, rules, transport, None).is_empty());
        assert_eq!(frames(&sim, rules, transport, Some(transport)), empty);
    }
}

/// Replays the native two-passenger fractional Size controls through the
/// production cargo and PIPS2 owners. The native rows patch the passenger's
/// current Type Size after reading; this fixture applies the same binary64
/// field override after Rules construction, then boards two live typed units.
#[test]
fn fractional_passenger_pips_match_original_cargo_and_draw_controls() {
    use crate::rules::ini_parser::IniFile;
    use crate::rules::ruleset::RuleSet;
    use crate::sim::passenger::{PassengerCargo, PassengerRole};

    let native = crate::test_fixture::json("tools/procedural_drawing_oracle/passenger_pips.json");
    for control in [
        "fractional_size_1_5_two_passengers",
        "fractional_size_2_8_two_passengers",
    ] {
        let row = native["raster_controls"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["name"] == control)
            .unwrap_or_else(|| panic!("missing native control {control}"));
        let size_patch = &row["input"]["current_type_size_override_after_read"];
        let passenger_type = size_patch["type_name"].as_str().unwrap();
        let carrier_type = row["input"]["actors"][0]["type_name"].as_str().unwrap();
        let size_bits = u64::from_str_radix(size_patch["f64_bits"].as_str().unwrap(), 16).unwrap();
        let passenger_size = f64::from_bits(size_bits);
        let mut rules = RuleSet::from_ini(&IniFile::from_str(&format!(
            "[VehicleTypes]\n0={carrier_type}\n1={passenger_type}\n\
             [{carrier_type}]\nStrength=100\nArmor=heavy\nSpeed=4\nPassengers=12\n\
             PipScale=Passengers\nSizeLimit=2\n\
             [{passenger_type}]\nStrength=100\nArmor=heavy\nSpeed=4\n"
        )))
        .expect("fractional carrier and passenger rules");
        apply_native_size_override_for_test(&mut rules, size_patch);

        let mut sim = Simulation::new();
        crate::sim::arena_fixture::flat_arena(&mut sim, &rules);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let carrier = sim
            .spawn_object(carrier_type, OWNER, 16, 16, 64, &rules)
            .expect("carrier fixture");
        let first = sim
            .spawn_object(passenger_type, OWNER, 16, 15, 64, &rules)
            .expect("first passenger fixture");
        let second = sim
            .spawn_object(passenger_type, OWNER, 17, 15, 64, &rules)
            .expect("second passenger fixture");
        let mut cargo = PassengerCargo::new(12, 2.0);
        cargo.board_forced_chain(&[(first, passenger_size), (second, passenger_size)]);
        assert_eq!(
            cargo.total_size(),
            row["actor_results"][0]["cargo_size"].as_i64().unwrap() as i32,
            "{}",
            control
        );
        sim.entities_mut_for_test()
            .get_mut(carrier)
            .unwrap()
            .passenger_role = PassengerRole::transport(cargo);
        assert!(sim.apply_command(
            OWNER,
            &Command::Select {
                entity_ids: vec![carrier],
                additive: false,
            },
            Some(&rules)
        ));

        let expected: Vec<_> = row["draws"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|draw| draw["asset"] == "PIPS2.SHP")
            .map(|draw| draw["frame"].as_u64().unwrap() as usize)
            .collect();
        assert_eq!(frames(&sim, &rules, carrier, None), expected, "{control}");
    }
}

/// A Yuri Slave has no Miner; its Storage is its SlaveLink's hold. The
/// storage arm (`0x0070A1E9..0x0070A340`) reads it as it reads a War
/// Miner's: retail SLAV is `PipScale=Tiberium` with `Storage=4`, three pips
/// for an infantryman (`0x007162BD`). Expected frames come from the shared
/// rounding kernel the HARV rows above pin, not from a native SLAV run.
#[test]
fn a_slave_shows_the_load_it_carries() {
    use crate::sim::miner::{CargoBale, ResourceType};
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    let rules = &retail.rules;
    let slave_type = rules.object("SLAV").unwrap();
    assert_eq!(
        (slave_type.pip_scale, slave_type.storage),
        (crate::rules::object_type::PipScale::Tiberium, 4)
    );
    let mut sim = Simulation::new();
    crate::sim::arena_fixture::flat_arena(&mut sim, rules);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    let slave = sim.spawn_object("SLAV", OWNER, 16, 16, 64, rules).unwrap();
    assert!(sim.entities().get(slave).unwrap().miner.is_none());
    assert!(sim.apply_command(
        OWNER,
        &Command::Select {
            entity_ids: vec![slave],
            additive: false,
        },
        Some(rules)
    ));
    for (ore, gems, expected) in [
        (0, 0, [0, 0, 0]),
        (1, 0, [2, 0, 0]),
        (3, 0, [2, 2, 0]),
        (4, 0, [2, 2, 2]),
        (2, 2, [5, 5, 2]),
    ] {
        let bale = |resource_type| CargoBale {
            resource_type,
            value: 0,
        };
        let cargo = std::iter::repeat_n(bale(ResourceType::Ore), ore)
            .chain(std::iter::repeat_n(bale(ResourceType::Gem), gems))
            .collect();
        sim.entities_mut_for_test().get_mut(slave).unwrap().slave =
            crate::sim::slave_manager::SlaveLink::for_test(None, cargo);
        assert_eq!(
            frames(&sim, rules, slave, None),
            expected,
            "{ore} ore, {gems} gems"
        );
    }
}

/// Controlled storage-consumer priors; this does not establish harvesting.
/// Native executions supply expected frames, including PC53/chop half ties.
#[test]
fn shared_harvester_scale_matches_native_independent_ore_and_gem_counts() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    let rules = &retail.rules;
    let mut sim = Simulation::new();
    crate::sim::arena_fixture::flat_arena(&mut sim, rules);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    let miner = sim.spawn_object("HARV", OWNER, 16, 16, 64, rules).unwrap();
    assert!(sim.apply_command(
        OWNER,
        &Command::Select {
            entity_ids: vec![miner],
            additive: false,
        },
        Some(rules)
    ));
    let native = crate::test_fixture::json("tools/procedural_drawing_oracle/passenger_pips.json");
    for row in native["raster_controls"].as_array().unwrap() {
        let actor = &row["input"]["actors"][0];
        if actor["type_name"] != "HARV" {
            continue;
        }
        crate::sim::arena_fixture::install_status_miner_cargo(
            sim.entities_mut_for_test().get_mut(miner).unwrap(),
            rules.object("HARV").unwrap(),
            rules,
            actor["ore_bales"].as_u64().unwrap() as usize,
            actor["gem_bales"].as_u64().unwrap() as usize,
        );
        let expected: Vec<_> = row["draws"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|draw| draw["asset"] == "PIPS2.SHP")
            .map(|draw| draw["frame"].as_u64().unwrap() as usize)
            .collect();
        assert_eq!(
            frames(&sim, rules, miner, None),
            expected,
            "{}",
            row["name"]
        );
    }
}
