//! AircraftClass::AI414DA8..414DB0 stops after a lethal Foot AI callback,
//! before the sinking descent and wake suffix at414DB6..414ED0.

use super::*;

/// Supplied callback-boundary prior: living sink Stun runs before AttachTo.
/// Ordinary attacks reject sinking targets, and a pre-sink infection is cleared
/// by Stun's self pointer expiry. No attack/warp producer is claimed here. Real
/// stock parasite callbacks still deliver the lethal damage during Foot AI.
#[test]
fn post_foot_dead_aircraft_skips_sink_suffix() {
    let Some((mut rules, mut sim, _)) = crate::sim::superweapon::chronosphere_tests::retail_world()
    else {
        return;
    };
    rules.bind_anim_frame_count_for_test("WAKE1", 20);
    let aircraft = sim
        .spawn_object_at_height("ORCA", "Americans", 21, 21, 0, 0, &rules)
        .expect("landed stock Harrier");
    sim.begin_living_ship_sinking(
        aircraft,
        LivingSinkTrigger::PostWarpValidation,
        &rules,
        FrameEffects::default(),
    );
    let drone = sim
        .spawn_object_at_height("DRON", "Russians", 22, 21, 0, 0, &rules)
        .expect("stock Terror Drone");
    // Supply AttachTo's prior after Stun, not a surviving pre-warp infection.
    sim.techno_limbo_with_rules(drone, &rules, None);
    assert!(sim.parasite_can_infect(drone, Some(aircraft), &rules));
    sim.parasite_attach(drone, Some(aircraft), &rules, FrameEffects::default());
    assert_eq!(
        sim.substrate
            .entities
            .get(aircraft)
            .unwrap()
            .parasite
            .eater(),
        Some(drone)
    );
    // Two real damage deliveries establish Health50 with the third due at121.
    for frame in [1, 61] {
        sim.session.set_binary_frame_for_test(frame);
        sim.parasite_ai_for_victim(aircraft, &rules, None, FrameEffects::default());
    }
    assert_eq!(
        sim.substrate
            .entities
            .get(aircraft)
            .unwrap()
            .health
            .current(),
        50
    );
    sim.session.set_binary_frame_for_test(120);
    assert!(!sim.tick_ship_sinking(aircraft, &rules, None, FrameEffects::default()));
    let before = position_world_coord(&sim.substrate.entities.get(aircraft).unwrap().position);
    assert_eq!(before.z, -5);
    let wake_count = |sim: &Simulation| {
        sim.substrate
            .anims
            .iter()
            .filter(|(_, anim)| sim.interner.resolve(anim.type_id()) == "WAKE1")
            .count()
    };
    let wakes_before = wake_count(&sim);
    // A mod-four visit would take both wake draws if the dead aircraft reached
    // its sink suffix. The source's ordinary Foot tail delivers this bite.
    sim.session.set_binary_frame_for_test(124);
    sim.advance_live_object_turn(
        aircraft,
        &rules,
        super::super::techno_ai::ObjectAiCtx::default(),
    )
    .expect("lethal parasite in the ordinary Foot tail");
    let actor = sim
        .substrate
        .entities
        .get(aircraft)
        .expect("deferred deletion until frame tail");
    assert!(!actor.lifecycle.object_alive());
    assert!(sim.substrate.pending_delete.contains_object(&aircraft));
    assert_eq!(position_world_coord(&actor.position), before);
    assert_eq!(wake_count(&sim), wakes_before);
}
