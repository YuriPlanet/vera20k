//! Stock Bio Reactor cargo feeds the shared native pip consumer. Entry and
//! departure use the existing production commands and ordinary frame owner.
use crate::rules::ruleset::RuleSet;
use crate::sim::command::Command;
use crate::sim::world::{FrameEffects, Simulation, TickLane};
use std::collections::BTreeMap;

const OWNER: &str = "YuriCountry";

fn frames(sim: &Simulation, rules: &RuleSet, reactor: u64, hover: Option<u64>) -> Vec<usize> {
    let entity = sim.entities().get(reactor).unwrap();
    let before = (
        sim.state_hash(),
        sim.main_rng.native_state_hex(),
        sim.scenario_rng.native_state_hex(),
        sim.mapgen_rng.native_state_hex(),
        entity
            .passenger_role
            .cargo()
            .map(|cargo| (cargo.passengers().to_vec(), cargo.total_size())),
    );
    let result = super::ui_overlays::scale_pip_draws(sim, entity, hover, Some(OWNER), rules)
        .into_iter()
        .map(|pip| pip.frame)
        .collect();
    assert_eq!(
        before,
        (
            sim.state_hash(),
            sim.main_rng.native_state_hex(),
            sim.scenario_rng.native_state_hex(),
            sim.mapgen_rng.native_state_hex(),
            entity
                .passenger_role
                .cargo()
                .map(|cargo| (cargo.passengers().to_vec(), cargo.total_size())),
        ),
        "pip queries leave cargo, simulation state and RNG untouched"
    );
    result
}

fn native_frames(cargo_types: &[&str]) -> Vec<usize> {
    let native = crate::test_fixture::json("tools/procedural_drawing_oracle/absorber_pips.json");
    let row = native["raster_controls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| {
            let actors = row["input"]["actors"].as_array().unwrap();
            actors.len() == 1
                && actors[0]["type_name"] == "YAPOWR"
                && actors[0]["selected"] == true
                && actors[0]["owner_is_current"].as_bool().unwrap_or(true)
                && row["input"]["scenario_patch"].is_null()
                && row["actor_results"][0]["cargo_types"]
                    .as_array()
                    .is_some_and(|types| {
                        types
                            .iter()
                            .filter_map(|kind| kind.as_str())
                            .collect::<Vec<_>>()
                            == cargo_types
                    })
        })
        .expect("original stock Bio Reactor draws this cargo-head order");
    let slots = row["actor_results"][0]["pip_max"].as_u64().unwrap() as usize;
    let draws = row["draws"].as_array().unwrap();
    let frames: Vec<_> = draws
        .iter()
        .filter(|draw| draw["pip_tail"] == true)
        .map(|draw| {
            assert_eq!(draw["asset"], "PIPS.SHP");
            assert_eq!(draw["flags"], 0x600);
            draw["frame"].as_u64().unwrap() as usize
        })
        .collect();
    assert_eq!(frames.len(), slots);
    frames
}

fn advance(sim: &mut Simulation, rules: &RuleSet) {
    sim.advance_app_frame(
        &[],
        Some(rules),
        None,
        33,
        TickLane::Ordinary,
        FrameEffects::default(),
    )
    .expect("ordinary stock Bio Reactor frame");
}

#[test]
fn stock_bio_reactor_pips_follow_boarding_cargo_head_departure_and_selection() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    let rules = &retail.rules;
    let expected: BTreeMap<Vec<&str>, Vec<usize>> = [
        (vec![], native_frames(&[])),
        (vec!["INIT"], native_frames(&["INIT"])),
        (
            vec!["YENGINEER", "INIT"],
            native_frames(&["YENGINEER", "INIT"]),
        ),
    ]
    .into();
    let empty = &expected[&Vec::<&str>::new()];
    let mut sim = Simulation::with_seed(1);
    crate::sim::arena_fixture::flat_arena(&mut sim, rules);
    for (name, human) in [(OWNER, true), ("Americans", false)] {
        let owner = sim.interner.intern(name);
        sim.houses
            .register(crate::sim::house_state::HouseState::new(
                owner, 0, None, human, 0, 10,
            ));
    }
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    let owner = sim.interner.get(OWNER).unwrap();
    sim.fog.reveal_all_for_owner(owner);
    let reactor = sim
        .spawn_object("YAPOWR", OWNER, 16, 16, 64, rules)
        .unwrap();
    let initiate = sim.spawn_object("INIT", OWNER, 12, 16, 64, rules).unwrap();
    let engineer = sim
        .spawn_object("YENGINEER", OWNER, 12, 18, 64, rules)
        .unwrap();
    sim.spawn_object("GAPOWR", "Americans", 25, 25, 64, rules)
        .unwrap();
    assert!(sim.apply_command(
        OWNER,
        &Command::Select {
            entity_ids: vec![reactor],
            additive: false
        },
        Some(rules),
    ));
    assert_eq!(
        rules.object("YAPOWR").unwrap().pip_max(rules) as usize,
        empty.len()
    );
    assert_eq!(&frames(&sim, rules, reactor, None), empty);

    for (passenger, types) in [
        (initiate, vec!["INIT"]),
        (engineer, vec!["YENGINEER", "INIT"]),
    ] {
        assert!(sim.apply_command(
            OWNER,
            &Command::EnterTransport {
                passenger_id: passenger,
                transport_id: reactor
            },
            Some(rules),
        ));
        for _ in 0..480 {
            advance(&mut sim, rules);
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
        assert_eq!(rider.passenger_role.inside_transport_id(), Some(reactor));
        assert!(rider.lifecycle.in_limbo());
        assert_eq!(&frames(&sim, rules, reactor, None), &expected[&types]);
    }
    assert_eq!(
        sim.entities()
            .get(reactor)
            .unwrap()
            .passenger_role
            .cargo()
            .unwrap()
            .passengers(),
        [engineer, initiate],
        "the shared CargoClass owner prepends each admitted passenger"
    );
    assert_eq!(
        &frames(&sim, rules, reactor, None),
        &expected[&vec!["YENGINEER", "INIT"]]
    );
    assert!(sim.apply_command(
        OWNER,
        &Command::UnloadPassengers {
            transport_id: reactor
        },
        Some(rules),
    ));
    for _ in 0..480 {
        advance(&mut sim, rules);
        let cargo = sim
            .entities()
            .get(reactor)
            .unwrap()
            .passenger_role
            .cargo()
            .unwrap();
        let types: Vec<_> = cargo
            .passengers()
            .iter()
            .map(|id| {
                sim.interner
                    .resolve(sim.entities().get(*id).unwrap().type_ref())
            })
            .collect();
        assert_eq!(&frames(&sim, rules, reactor, None), &expected[&types]);
        if cargo.is_empty() {
            break;
        }
    }
    assert!(
        sim.entities()
            .get(reactor)
            .unwrap()
            .passenger_role
            .cargo()
            .unwrap()
            .is_empty()
    );
    for passenger in [engineer, initiate] {
        let rider = sim.entities().get(passenger).unwrap();
        assert!(!rider.passenger_role.is_inside_transport());
        assert!(!rider.lifecycle.in_limbo());
    }
    assert_eq!(&frames(&sim, rules, reactor, None), empty);
    assert!(sim.apply_command(
        OWNER,
        &Command::Select {
            entity_ids: vec![],
            additive: false
        },
        Some(rules),
    ));
    assert!(frames(&sim, rules, reactor, None).is_empty());
    assert_eq!(&frames(&sim, rules, reactor, Some(reactor)), empty);
}
