//! VERA `[JumpjetControls] SmoothFacing=`: a Nighthawk's body facing through
//! a take-off, an unload and the next take-off, with the key off (the
//! original's snaps) and on.

use crate::rules::ini_parser::IniFile;
use crate::sim::combat::TargetKind;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::mission::MissionType;
use crate::sim::movement::jumpjet_movement::jumpjet_flight::STATE_GROUND;
use crate::sim::world::Simulation;

/// The largest change of the body facing between two frames, in 1/256 turns,
/// while a GI boards a landed Nighthawk, it flies west, unloads and flies
/// back. `None` without retail rules.
fn largest_turn_in_a_frame(patch: &str) -> Option<i32> {
    let retail = crate::rules::retail_ini_fixture::retail_battle_rules_for_map_with_patch(
        "Hills.mmx",
        &IniFile::from_str(patch),
    )?;
    let rules = &retail.rules;
    let mut sim = Simulation::new();
    for (name, side) in [("Americans", 0), ("Russians", 1)] {
        let house = sim.intern(name);
        sim.houses_mut_for_test()
            .register(crate::sim::house_state::HouseState::new(
                house, side, None, true, 0, 10,
            ));
    }
    let owner = sim.interner().get("Americans").unwrap();
    sim.session_mut_for_test()
        .set_current_house_for_test(Some(owner));
    sim.session_mut_for_test()
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    crate::sim::arena_fixture::flat_arena(&mut sim, rules);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    // Each House keeps a building, so the skirmish has no winner.
    sim.spawn_object("NAPOWR", "Russians", 3, 3, 0, rules)
        .unwrap();
    sim.spawn_object("GAPOWR", "Americans", 16, 26, 0, rules)
        .unwrap();
    let hawk = sim
        .spawn_object("SHAD", "Americans", 20, 18, 0, rules)
        .unwrap();
    let gi = sim
        .spawn_object("E1", "Americans", 21, 18, 0, rules)
        .unwrap();
    sim.resolve_type_handles(rules);

    let facing = |sim: &Simulation| {
        let frame = sim.session().binary_frame();
        sim.entities().get(hawk).unwrap().body_facing.current(frame)
    };
    let landed = |sim: &Simulation| {
        let entity = sim.entities().get(hawk).unwrap();
        entity.navigation.nav_com().is_none()
            && entity
                .locomotor
                .as_ref()
                .and_then(|locomotor| locomotor.jumpjet_runtime())
                .is_some_and(|runtime| runtime.phase() == STATE_GROUND)
    };
    let mut largest = 0;
    // Queue `order`, then run frames until `done` holds after at least one.
    let mut run = |sim: &mut Simulation, order: Command, done: &dyn Fn(&Simulation) -> bool| {
        sim.queue_command(CommandEnvelope::new(owner, sim.session().tick(), order));
        for frame in 0..800 {
            let before = facing(sim);
            let due = sim.take_due_commands();
            let result = sim.advance_tick(&due, Some(rules), None, None, 67);
            assert!(result.frame_committed, "{result:?}");
            let turned = i32::from(facing(sim).wrapping_sub(before) as i16).abs() / 256;
            largest = largest.max(turned);
            if frame > 20 && done(sim) {
                return;
            }
        }
        panic!("the step did not finish");
    };
    let move_to = |rx, ry| Command::Move {
        entity_id: hawk,
        target_rx: rx,
        target_ry: ry,
    };
    run(
        &mut sim,
        Command::MegaMission {
            entity_id: gi,
            mission: MissionType::Enter,
            target: None,
            destination: Some(TargetKind::Entity(hawk)),
        },
        &|sim| {
            sim.entities()
                .get(gi)
                .is_some_and(|gi| gi.passenger_role.is_inside_transport())
        },
    );
    run(&mut sim, move_to(8, 18), &landed);
    run(
        &mut sim,
        Command::UnloadPassengers { transport_id: hawk },
        &|sim| {
            let entity = sim.entities().get(hawk).unwrap();
            entity.passenger_role.passenger_count() == 0
                && entity.mission.current().known() == Some(MissionType::Guard)
        },
    );
    run(&mut sim, move_to(20, 18), &landed);
    Some(largest)
}

/// The original snaps the body in one frame: to the locomotor's link-time
/// east on the first take-off (`0x0054D67B`), to the exit direction for an
/// Unload (`0x0054B6E0`) and back on the next take-off. With the key on every
/// turn is gradual.
#[test]
fn smooth_facing_turns_where_the_original_snaps() {
    let (Some(original), Some(smooth)) = (
        largest_turn_in_a_frame(""),
        largest_turn_in_a_frame("[JumpjetControls]\nSmoothFacing=yes\n"),
    ) else {
        return;
    };
    assert!(original >= 32, "the original snaps: {original}");
    assert!(smooth <= 8, "SmoothFacing turns gradually: {smooth}");
}
