//! VERA `[JumpjetControls] LandNearby=`: a Nighthawk whose landing cell is
//! taken while it descends, with the key off (the original's hover) and on.

use crate::rules::ini_parser::IniFile;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::movement::jumpjet_movement::jumpjet_flight::{STATE_DESCEND, STATE_GROUND};
use crate::sim::world::Simulation;

const LANDING: (u16, u16) = (20, 18);

/// Where the Nighthawk stands after a tank appeared on its landing cell during
/// the descent, and whether it got down. `None` without retail rules.
fn after_a_blocked_landing(patch: &str) -> Option<((u16, u16), bool)> {
    let (cell, landed_after) = blocked_landing(patch, Block::DuringDescent)?;
    Some((cell, landed_after.is_some()))
}

/// When the tank takes the landing cell.
#[derive(Clone, Copy, PartialEq)]
enum Block {
    Never,
    /// Two frames after the order, long before the Nighthawk arrives.
    Early,
    DuringDescent,
}

/// The Nighthawk's cell at the end, and the frames from its order to its
/// touchdown when it landed within 1000 frames. `None` without retail rules.
fn blocked_landing(patch: &str, block: Block) -> Option<((u16, u16), Option<u32>)> {
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
        .spawn_object("SHAD", "Americans", 10, 18, 0, rules)
        .unwrap();
    sim.resolve_type_handles(rules);

    let phase = |sim: &Simulation| {
        sim.entities()
            .get(hawk)
            .and_then(|entity| entity.locomotor.as_ref())
            .and_then(|locomotor| locomotor.jumpjet_runtime())
            .expect("Jumpjet")
            .phase()
    };
    let frame = |sim: &mut Simulation| {
        let due = sim.take_due_commands();
        let result = sim.advance_tick(&due, Some(rules), None, None, 67);
        assert!(result.frame_committed, "{result:?}");
    };
    sim.queue_command(CommandEnvelope::new(
        owner,
        sim.session().tick(),
        Command::Move {
            entity_id: hawk,
            target_rx: LANDING.0,
            target_ry: LANDING.1,
        },
    ));
    let mut blocked = block == Block::Never;
    let mut landed_after = None;
    let mut left_the_ground = false;
    for frames in 0..1000 {
        frame(&mut sim);
        let now = phase(&sim);
        if !blocked
            && (block == Block::Early && frames == 2
                || block == Block::DuringDescent && now == STATE_DESCEND)
        {
            sim.spawn_object("MTNK", "Americans", LANDING.0, LANDING.1, 0, rules)
                .unwrap();
            sim.resolve_type_handles(rules);
            blocked = true;
        }
        left_the_ground |= now != STATE_GROUND;
        if left_the_ground && now == STATE_GROUND {
            landed_after = Some(frames);
            break;
        }
    }
    let position = sim.entities().get(hawk).unwrap().position;
    Some(((position.rx(), position.ry()), landed_after))
}

/// The original hovers over a landing cell something else took
/// (`State4_Descend`'s refusal, `0x0054C550`). With `LandNearby=3` the
/// Nighthawk lands on a clear cell beside it.
#[test]
fn land_nearby_lands_beside_a_taken_cell() {
    let (Some(original), Some(nearby)) = (
        after_a_blocked_landing(""),
        after_a_blocked_landing("[JumpjetControls]\nLandNearby=3\n"),
    ) else {
        return;
    };
    assert_eq!(original, (LANDING, false), "the original hovers");
    let (cell, landed) = nearby;
    assert!(landed, "LandNearby lands: {cell:?}");
    let (dx, dy) = (
        i32::from(cell.0) - i32::from(LANDING.0),
        i32::from(cell.1) - i32::from(LANDING.1),
    );
    assert!(cell != LANDING && dx * dx + dy * dy <= 9, "{cell:?}");
}

/// A cell taken while the Nighthawk is still on its way is replaced on the
/// approach, so the landing takes about as long as an unhindered one rather
/// than a descent, a refusal and a second flight.
#[test]
fn land_nearby_replans_on_the_approach() {
    let key = "[JumpjetControls]\nLandNearby=3\n";
    let (Some(clear), Some(early)) = (
        blocked_landing(key, Block::Never),
        blocked_landing(key, Block::Early),
    ) else {
        return;
    };
    let (Some(clear), (cell, Some(replanned))) = (clear.1, early) else {
        panic!("both Nighthawks land: {clear:?} {early:?}");
    };
    assert_ne!(cell, LANDING);
    assert!(replanned <= clear + 20, "{replanned} against {clear}");
}
