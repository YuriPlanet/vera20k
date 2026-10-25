//! TAction44/45/90/102 reach the existing Lightning Storm owner, whose Start539EB0
//! and CreateCloudBolt53A140 ports carry their own native comparisons
//! (superweapon::lightning_storm_tests). These cover the dispatch inputs.

use super::*;
use crate::map::waypoints::Waypoint;
use crate::sim::superweapon::lightning_storm;
use std::collections::HashMap;

fn action(kind: i32, waypoint: u32) -> RuntimeAction {
    let entry = crate::map::actions::ActionEntry {
        kind,
        params: vec!["0".into(); 6],
        waypoint_index: Some(waypoint),
    };
    RuntimeDefinitions::default().resolve_action(entry, None, None)
}

fn run(
    sim: &mut Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    action: &RuntimeAction,
    house: Option<crate::sim::intern::InternedId>,
) -> bool {
    sim.apply_live_trigger_action(
        action,
        TriggerActionContext {
            live_index: None,
            incoming_house: house,
            input: TagEventInput::default(),
        },
        &mut Vec::new(),
        rules,
        None,
        FrameEffects::default(),
    )
}

fn world() -> Option<(
    crate::rules::ruleset::RuleSet,
    Simulation,
    crate::sim::intern::InternedId,
)> {
    let rules = crate::sim::superweapon::lightning_storm_tests::retail_rules()?;
    let (rules, mut sim, americans) =
        crate::sim::superweapon::chronosphere_tests::world_with(rules, 64, &[]);
    sim.session.install_waypoints(&HashMap::from([(
        7,
        Waypoint {
            index: 7,
            rx: 30,
            ry: 31,
        },
    )]));
    Some((rules, sim, americans))
}

#[test]
fn lightning_strike_action_creates_one_cloud_bolt_over_the_waypoint() {
    let Some((rules, mut sim, _)) = world() else {
        return;
    };
    assert!(run(&mut sim, &rules, &action(90, 7), None));
    let (present, manifesting) = sim.lightning_storm.clouds_for_test();
    assert_eq!((present.len(), manifesting.len()), (1, 1));
    let cloud = sim.anim(present[0]).unwrap();
    assert!(
        rules
            .general
            .weather_con_clouds
            .iter()
            .any(|name| name.eq_ignore_ascii_case(sim.interner.resolve(cloud.type_id())))
    );
    // 53A140 places the cloud over the cell centre; no storm is started.
    assert_eq!(
        (cloud.world_coord().x, cloud.world_coord().y),
        (30 * 256 + 128, 31 * 256 + 128)
    );
    assert!(!lightning_storm::has_deferment(&sim));
}

#[test]
fn lightning_storm_action_starts_once_for_the_incoming_house() {
    let Some((rules, mut sim, americans)) = world() else {
        return;
    };
    assert!(run(&mut sim, &rules, &action(102, 7), Some(americans)));
    let (_, _, deferment, ..) = sim.lightning_storm.globals_for_test();
    assert_eq!(deferment, rules.general.lightning_deferment);
    assert_eq!(sim.lightning_storm.owner(), Some(americans));
    // The storm counts down and starts through the shared owner.
    let mut frames = 0;
    while !lightning_storm::raging(&sim) {
        crate::sim::superweapon::chronosphere_tests::step(&mut sim, &rules);
        frames += 1;
        assert!(frames <= rules.general.lightning_deferment);
    }
    let (.., cell, _) = sim.lightning_storm.globals_for_test();
    assert_eq!(cell, (30, 31));
    // 6DDA98: a raging storm makes the action fail without a new Start.
    assert!(!run(&mut sim, &rules, &action(102, 7), None));
    assert_eq!(sim.lightning_storm.owner(), Some(americans));
}

fn numeric_action(kind: i32, value: &str) -> RuntimeAction {
    let ini = crate::rules::ini_parser::IniFile::from_str(&format!(
        "[Actions]\nCONTROL=1,{kind},0,{value},0,0,0,0,A\n"
    ));
    let entry = crate::map::actions::parse_actions(&ini)
        .remove("CONTROL")
        .unwrap()
        .entries()[0]
        .clone();
    RuntimeDefinitions::default().resolve_action(entry, None, None)
}

fn scenario_hash(sim: &Simulation) -> String {
    crate::util::sha256::sha256_hex(sim.scenario_rng.native_state_hex().as_bytes())
}

#[test]
fn legacy_storm_actions_match_original_empty_target_rng_and_countdown_gates() {
    use lightning_storm::LightningStorm;
    // Original Execute44/45, static EmptyCell initializer6DCF10, Start539EB0
    // and Scenario RNG (Steam gamemd). Declared map/House priors reuse
    // the existing storm oracle fixture; no incoming-House attribution.
    for (seed, size, raw, duration, target, expected_rng) in [
        (
            7,
            (60, 60),
            "180",
            180,
            (67, 99),
            "96a45dec6480d4543b4a6a61e87610de3f86182aa4d45634359e718d497cd9a0",
        ),
        (
            3,
            (8, 8),
            "2147483647",
            i32::MAX,
            (6, 6),
            "02322a530047efb8887aaac370f257aa47037c12e85be7495b80178e8be6969c",
        ),
    ] {
        let (rules, mut sim, houses) =
            crate::sim::superweapon::lightning_storm_tests::trigger_control_fixture(seed, size);
        sim.lightning_storm =
            LightningStorm::for_test(false, false, 0, -1, 900, (7, 8), Some(houses[0]));
        let main = sim.main_rng.native_state_hex();
        let mapgen = sim.mapgen_rng.native_state_hex();
        assert!(run(
            &mut sim,
            &rules,
            &numeric_action(44, raw),
            Some(houses[0])
        ));
        assert_eq!(
            sim.lightning_storm.globals_for_test(),
            (false, false, 250, duration, 900, target, None)
        );
        assert_eq!(scenario_hash(&sim), expected_rng);
        let hash = sim.state_hash();
        assert!(!run(
            &mut sim,
            &rules,
            &numeric_action(45, "0"),
            Some(houses[0])
        ));
        assert_eq!(sim.state_hash(), hash, "45 cannot cancel the countdown");
        assert_eq!(scenario_hash(&sim), expected_rng);
        if seed == 7 {
            assert!(run(
                &mut sim,
                &rules,
                &numeric_action(44, "-1"),
                Some(houses[0])
            ));
            assert_eq!(
                sim.lightning_storm.globals_for_test(),
                (false, false, 250, -1, 900, (64, 93), None)
            );
            assert_eq!(
                scenario_hash(&sim),
                "1de1aa00c5674f47b3cfa6d1a509fc4b84913f2b2ef079b99d10b6153210f597"
            );
        }
        assert_eq!(sim.main_rng.native_state_hex(), main);
        assert_eq!(sim.mapgen_rng.native_state_hex(), mapgen);
    }
}

#[test]
fn legacy_storm_actions_immediate_start_null_owner_and_stop_are_native_retained_writes() {
    let (mut rules, mut sim, houses) =
        crate::sim::superweapon::lightning_storm_tests::trigger_control_fixture(11, (4, 6));
    rules.general.lightning_deferment = 0;
    assert!(run(
        &mut sim,
        &rules,
        &numeric_action(44, "0"),
        Some(houses[0])
    ));
    let before = sim.lightning_storm.globals_for_test();
    assert_eq!(before, (true, false, 0, 0, 1000, (4, 4), None));
    assert_eq!(
        scenario_hash(&sim),
        "7b004992331b906dc314b9c5d7615d7d6d387a249a7413b2947190c16cbdf405"
    );
    // Both live allies get an outage because 44 passes NULL, even with an
    // incoming House; the defeated House is skipped by the shared Start.
    for (index, owner) in houses.iter().enumerate() {
        let timer = sim.power_states[owner].radar_outage_for_test();
        assert_eq!(
            (timer.start_frame(), timer.duration()),
            if index < 2 { (1000, 0) } else { (0, 0) }
        );
    }
    let rng = sim.rng_state();
    let hash = sim.state_hash();
    assert!(!run(&mut sim, &rules, &numeric_action(44, "99"), None));
    assert_eq!(sim.state_hash(), hash);
    assert!(run(&mut sim, &rules, &numeric_action(45, "0"), None));
    let mut wanted = before;
    wanted.1 = true;
    assert_eq!(sim.lightning_storm.globals_for_test(), wanted);
    assert_ne!(
        sim.state_hash(),
        hash,
        "pending end affects the next Process"
    );
    let hash = sim.state_hash();
    assert!(run(&mut sim, &rules, &numeric_action(45, "0"), None));
    assert_eq!(
        sim.state_hash(),
        hash,
        "repeated active Stop succeeds without another change"
    );
    assert_eq!(sim.rng_state(), rng);
}

#[test]
fn legacy_storm_stop_keeps_retail_clouds_until_production_process_finishes_them() {
    let Some((rules, mut sim, owner)) = world() else {
        return;
    };
    let rng = sim.rng_state();
    assert!(!run(&mut sim, &rules, &numeric_action(45, "0"), None));
    assert_eq!(sim.rng_state(), rng);
    sim.lightning_storm = lightning_storm::LightningStorm::raging_for_test(owner, (30, 31));
    assert!(run(&mut sim, &rules, &action(90, 7), None));
    let (present, manifesting) = sim.lightning_storm.clouds_for_test();
    let before = (present.to_vec(), manifesting.to_vec());
    assert_eq!((before.0.len(), before.1.len()), (1, 1));
    let rng = sim.rng_state();
    assert!(run(&mut sim, &rules, &numeric_action(45, "0"), None));
    let (present, manifesting) = sim.lightning_storm.clouds_for_test();
    assert_eq!(
        (present, manifesting),
        (before.0.as_slice(), before.1.as_slice())
    );
    assert!(lightning_storm::raging(&sim));
    assert_eq!(sim.lightning_storm.owner(), Some(owner));
    assert_eq!(sim.rng_state(), rng);
    let saved = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "Stop pending", 0);
    let loaded = crate::sim::snapshot::GameSnapshot::load(&saved)
        .unwrap()
        .sim;
    assert_eq!(
        loaded.lightning_storm.globals_for_test(),
        sim.lightning_storm.globals_for_test()
    );
    for _ in 0..500 {
        crate::sim::superweapon::chronosphere_tests::step(&mut sim, &rules);
        assert!(sim.lightning_storm.clouds_for_test().0.len() <= 1);
        if !lightning_storm::raging(&sim) {
            break;
        }
    }
    assert!(!lightning_storm::raging(&sim));
    assert_eq!(sim.lightning_storm.owner(), None);
    assert_eq!(sim.lightning_storm.globals_for_test().5, (0, 0));
    assert!(!sim.lightning_storm.globals_for_test().1);
    assert!(sim.lightning_storm.clouds_for_test().0.is_empty());
    assert!(!run(&mut sim, &rules, &numeric_action(45, "0"), None));
}
