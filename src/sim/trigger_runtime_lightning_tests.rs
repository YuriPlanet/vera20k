//! TAction90/102 reach the existing Lightning Storm owner, whose Start539EB0
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
        Some(rules),
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
            .any(|name| name.eq_ignore_ascii_case(sim.interner.resolve(cloud.type_id)))
    );
    // 53A140 places the cloud over the cell centre; no storm is started.
    assert_eq!(
        (cloud.world_coord.x, cloud.world_coord.y),
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
