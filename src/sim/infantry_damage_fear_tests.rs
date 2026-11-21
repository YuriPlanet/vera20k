//! ReceiveDamage517FA0: the sourced Fraidycat latch518C7B precedes the
//! Fearless gates518C94/518CC8. Exercise the receiver with retail actors.
use crate::rules::ini_parser::IniFile;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::{EntityDamageEvent, RAD_NO_ATTACKER, ReceiverCallFlags};
use crate::sim::movement::ground_pose::position_world_xy;
use crate::sim::world::{FrameEffects, Simulation, TickLane};

fn retail_fraidycat_fearless() -> Option<(Simulation, RuleSet, OverlayTypeRegistry, u64)> {
    let retail = crate::rules::retail_ini_fixture::retail_battle_rules_with_map_patch(
        &IniFile::from_str("[E1]\nFraidycat=yes\nFearless=yes\n"),
    )?;
    let registry = OverlayTypeRegistry::from_ini(&retail.processed_rules, Some(&retail.fixed_art));
    let rules = retail.rules;
    let object = rules.object("E1").unwrap();
    assert!(object.fraidycat && object.fearless);
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x518C7B);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::Skirmish);
    let actor = sim
        .spawn_object("E1", "Americans", 16, 16, 0, &rules)
        .unwrap();
    Some((sim, rules, registry, actor))
}

fn receive_hit(
    sim: &mut Simulation,
    rules: &RuleSet,
    registry: &OverlayTypeRegistry,
    actor: u64,
    source: Option<u64>,
) {
    let before = sim.substrate.entities.get(actor).unwrap().health.current();
    let source_house = source.map(|id| sim.substrate.entities.get(id).unwrap().owner());
    let warhead = sim.interner.intern("SA");
    sim.commit_direct_damage_receiver(
        rules,
        Some(registry),
        EntityDamageEvent::direct_receiver(
            actor,
            1,
            0,
            source.unwrap_or(RAD_NO_ATTACKER),
            source_house,
            warhead,
            ReceiverCallFlags {
                ignore_defenses: false,
                arg6: false,
            },
        ),
        &mut FrameEffects::default(),
    );
    let after = sim.substrate.entities.get(actor).unwrap().health.current();
    assert!(
        after > 0 && after < before,
        "the receiver must take a surviving hit"
    );
}

#[test]
fn sourced_fraidycat_damage_precedes_fearless_and_reaches_normal_escape() {
    let Some((mut sim, rules, registry, actor)) = retail_fraidycat_fearless() else {
        return;
    };
    let source = sim
        .spawn_object("E1", "Russians", 18, 16, 0, &rules)
        .unwrap();
    let origin = position_world_xy(&sim.substrate.entities.get(actor).unwrap().position);
    receive_hit(&mut sim, &rules, &registry, actor, Some(source));
    assert_eq!(
        sim.substrate
            .entities
            .get(actor)
            .unwrap()
            .infantry
            .as_ref()
            .unwrap()
            .fear_level(),
        300
    );
    assert!(
        sim.substrate
            .entities
            .get(actor)
            .unwrap()
            .navigation
            .nav_com()
            .is_none(),
        "this human's unforced damage Scatter must not start the escape"
    );
    // Run ordinary frames through Infantry AI and the existing forced Scatter/
    // locomotor owners, until an actual world position changes.
    for _ in 0..30 {
        sim.advance_master_frame(
            &[],
            &rules,
            Some(&registry),
            67,
            TickLane::Ordinary,
            false,
            &mut FrameEffects::default(),
        )
        .unwrap();
        if position_world_xy(&sim.substrate.entities.get(actor).unwrap().position) != origin {
            break;
        }
    }
    let actor = sim.substrate.entities.get(actor).unwrap();
    assert_eq!(actor.infantry.as_ref().unwrap().fear_level(), 300);
    assert_ne!(
        position_world_xy(&actor.position),
        origin,
        "the retained latch must feed normal Fraidycat escape"
    );
}

#[test]
fn sourceless_fraidycat_damage_retains_the_fearless_gate() {
    let Some((mut sim, rules, registry, actor)) = retail_fraidycat_fearless() else {
        return;
    };
    receive_hit(&mut sim, &rules, &registry, actor, None);
    assert_eq!(
        sim.substrate
            .entities
            .get(actor)
            .unwrap()
            .infantry
            .as_ref()
            .unwrap()
            .fear_level(),
        0
    );
}
