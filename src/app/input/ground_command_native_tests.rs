//! Original MTNK input/Event/class comparisons at the app/simulation boundary.
//! Simulation fixture setup and native comparisons stay with their owners.

use super::{ordinary_cell_move_goal, roundtrip_ordinary_local_megamission};
use crate::rules::ruleset::RuleSet;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::intern::InternedId;
use crate::sim::world::{Simulation, factory_infantry_output_tests as native};

fn produce_move(
    sim: &Simulation,
    rules: &RuleSet,
    owner: InternedId,
    id: u64,
    clicked: (u16, u16),
    shift: bool,
) -> CommandEnvelope {
    let resolved = ordinary_cell_move_goal(sim, rules, owner, id, clicked, true, shift)
        .expect("original input emits an ordinary Move");
    assert!(!resolved.queue, "native Shift still emits ordinary Move");
    let issued = CommandEnvelope::new(
        owner,
        sim.session.tick,
        Command::Move {
            entity_id: id,
            target_rx: resolved.cell.0,
            target_ry: resolved.cell.1,
            queue: resolved.queue,
        },
    );
    let decoded = roundtrip_ordinary_local_megamission(sim, issued.clone()).unwrap();
    assert_eq!(decoded, issued, "ordinary native record codec");
    decoded
}

/// Query7404B0/click738910 -> ordinary Event -> Unit741970 returns, with
/// initialized component priors rather than whole construction/AI parity.
#[test]
fn initialized_unit_reissues_match_original_command_returns() {
    native::check_initialized_ground_reissues(&["unit_plain_A_B", "unit_shift_A_B"], produce_move);
}

#[test]
fn initialized_unit_shift_resolves_original_outside_playfield_cell_before_event() {
    native::check_initialized_unit_shift_resolver(produce_move);
}

#[test]
fn initialized_unit_same_and_later_reissues_match_original_command_returns() {
    native::check_initialized_ground_reissues(
        &[
            "unit_plain_A_A",
            "unit_shift_A_A",
            "unit_plain_A_later_B",
            "unit_shift_A_later_B",
        ],
        produce_move,
    );
}

#[test]
fn initialized_unit_paid_head_and_event_stop_reissues_match_original_returns() {
    native::check_initialized_unit_paid_reissues(produce_move);
}
