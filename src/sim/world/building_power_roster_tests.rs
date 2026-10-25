//! House508C30 uses its retained Building list, including during deferred death.

use super::{FrameEffects, PlacementEvidence, Simulation};
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::{EntityDamageEvent, RAD_NO_ATTACKER, ReceiverCallFlags};

fn hit(sim: &mut Simulation, rules: &RuleSet, target: u64) {
    let ap = sim.interner.intern("AP");
    sim.commit_direct_damage_receiver(
        rules,
        None,
        EntityDamageEvent::direct_receiver(
            target,
            rules.weapon("105mm").unwrap().damage,
            0,
            RAD_NO_ATTACKER,
            None,
            ap,
            ReceiverCallFlags {
                ignore_defenses: false,
                arg6: false,
            },
        ),
        FrameEffects::default(),
    );
}

fn sample(sim: &mut Simulation, id: u64) {
    Simulation::sample_building_health_for_house_update(
        &mut sim.substrate,
        &sim.houses,
        &mut sim.power_states,
        id,
    );
}

/// Real fatal damage removes House+68 at4FBC1C even when Explodes keeps
/// the marked body until its next AI. Another building's ordinary health
/// sample must assess the departed yard's drain as zero at508C8B..508D16.
#[test]
fn retail_destroyed_shipyard_leaves_power_before_its_marked_body_retires() {
    // This retail map enables shipyards; XMP03T4 explicitly disables them.
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP26S6.MAP")
    else {
        return;
    };
    let mut rules = retail.rules;
    let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let mut roots = crate::rules::effect_asset_catalog::tolerant_anim_class_roots(&rules);
    roots.extend(
        rules
            .general
            .damage_fire_types
            .iter()
            .map(|anim| anim.name.clone()),
    );
    rules.bind_anim_class_assets(&roots, &assets, "TEM", "TEMPERATE");
    rules.bind_effect_assets(&assets, "TEM", "TEMPERATE");
    let mut sim = crate::sim::arena_fixture::rival_arena_with_size(&rules, 0x508c30, 40);
    // This is a skirmish history, where House508CEE does not require the
    // current player's discovery byte as a campaign-controlled House does.
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::Skirmish);
    crate::sim::arena_fixture::flood(
        &mut sim,
        &rules,
        40,
        (7..30).flat_map(|y| (7..30).map(move |x| (x, y))),
    );
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let mut yards = Vec::new();
    for (x, y) in [(8, 8), (20, 20)] {
        let id = sim
            .construct_object_limbo_at_height(
                "GAYARD",
                "Americans",
                x,
                y,
                0,
                0,
                &rules,
                FrameEffects::default(),
            )
            .unwrap();
        assert!(
            sim.reveal_constructed_object_at_height(
                id,
                x,
                y,
                0,
                0,
                PlacementEvidence::EvaluateMark,
                &rules,
                FrameEffects::default(),
            )
            .is_some()
        );
        yards.push(id);
    }
    let (yard, peer) = (yards[0], yards[1]);
    let plant = sim
        .spawn_object("GAPOWR", "Americans", 3, 3, 0, &rules)
        .unwrap();
    // Ordinary repeated hits leave the plant producing between one and two
    // yards' drain. No health or power-cache state is supplied by the test.
    for _ in 0..32 {
        if sim.entities().get(plant).unwrap().health.current() <= 150 {
            break;
        }
        hit(&mut sim, &rules, plant);
    }
    for id in [yard, peer, plant] {
        sample(&mut sim, id);
    }
    let owner = sim.entities().get(yard).unwrap().owner();
    sim.assess_house_derived_state(owner, &rules, FrameEffects::default());
    let output = sim.power_states[&owner].total_output();
    assert!(
        (25..50).contains(&output),
        "the damaged plant supports one yard: {output}"
    );
    assert_eq!(sim.power_states[&owner].total_drain(), 50);
    assert!(sim.power_states[&owner].low_power());
    assert!(!sim.power_states[&owner].power_recheck_for_test());

    for _ in 0..40 {
        if sim.entities().get(yard).unwrap().health.current() == 0 {
            break;
        }
        hit(&mut sim, &rules, yard);
    }
    let retained = sim.entities().get(yard).unwrap();
    assert_eq!(retained.health.current(), 0);
    assert!(retained.lifecycle.object_alive() && retained.lifecycle.cell_marked());
    assert!(!retained.lifecycle.in_limbo() && !retained.death.is_dying());
    assert!(
        !sim.houses[&owner]
            .base_projection()
            .buildings()
            .contains(&yard)
    );
    assert!(
        sim.houses[&owner]
            .base_projection()
            .buildings()
            .contains(&peer)
    );

    hit(&mut sim, &rules, peer);
    sample(&mut sim, peer);
    assert!(sim.power_states[&owner].power_recheck_for_test());
    sim.assess_house_derived_state(owner, &rules, FrameEffects::default());
    assert_eq!(sim.power_states[&owner].total_output(), output);
    assert_eq!(sim.power_states[&owner].total_drain(), 25);
    assert!(!sim.power_states[&owner].low_power());
    // The next actual Building visit still owns the retained body's cleanup.
    sim.object_ai_visit_one_with_effects(yard, &rules, super::ObjectAiCtx::default());
    assert!(
        sim.entities()
            .get(yard)
            .is_none_or(|entity| entity.lifecycle.in_limbo())
    );
    sim.assess_house_derived_state(owner, &rules, FrameEffects::default());
    assert_eq!(sim.power_states[&owner].total_drain(), 25);
}
