//! Can the policy put a unit into a transport with a command?
//!
//! The 2-island map is only testable if a click on a transport boards it. The
//! mechanism is the native Enter mission plus docking; this test drives the
//! same command a player's click produces and watches the cargo hold.

use vera20k_core::rules::ruleset::RuleSet;
use vera20k_core::sim::command::{Command, CommandEnvelope};
use vera20k_core::sim::control::observation::Observation;
use vera20k_core::sim::intern::InternedId;
use vera20k_core::sim::world::Simulation;

fn arena() -> Option<(Simulation, RuleSet)> {
    let retail = vera20k_core::rules::retail_ini_fixture::retail_battle_rules()?;
    let rules = retail.rules;
    let mut sim = vera20k_core::sim::arena_fixture::rival_arena(&rules, 0xA17);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    Some((sim, rules))
}

fn step(sim: &mut Simulation, rules: &RuleSet, frames: usize) {
    for _ in 0..frames {
        let _ = sim.take_due_commands();
        sim.advance_tick(&[], Some(rules), None, None, 33);
    }
}

#[test]
fn a_click_on_a_transport_puts_the_infantry_aboard() {
    let Some((mut sim, rules)) = arena() else {
        return;
    };
    let americans: InternedId = sim.intern("Americans");
    let ifv = sim
        .spawn_object("FV", "Americans", 10, 10, 0, &rules)
        .expect("an IFV");
    let gi = sim
        .spawn_object("E1", "Americans", 12, 10, 0, &rules)
        .expect("a GI");
    sim.queue_commands(vec![CommandEnvelope::new(
        americans,
        sim.session().tick(),
        Command::MegaMission {
            entity_id: gi,
            mission: vera20k_core::sim::mission::MissionType::Enter,
            target: None,
            destination: Some(vera20k_core::sim::combat::TargetKind::Entity(ifv)),
        },
    )]);

    let mut boarded = 0;
    for _ in 0..240 {
        step(&mut sim, &rules, 1);
        let obs = Observation::new(&sim, americans);
        boarded = obs.passenger_count(ifv);
        if boarded > 0 {
            assert_eq!(
                obs.inside_transport(gi),
                Some(ifv),
                "the passenger names its carrier"
            );
            break;
        }
    }
    assert!(boarded > 0, "the infantry never boarded the IFV");
}
