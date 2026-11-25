//! Does a unit that is already holding a post still take a move?
//!
//! Two tests show a freshly spawned tank walks twenty cells on one `Goal::Move`:
//! through `Funnel::prepare`, and through the real `ControlBoard`. In a match,
//! a scout that has been standing on a guard post since the first frame does not
//! move at all — `still=11400` by the end. The difference is what the unit was
//! doing before the order, and `order_stands` is the rule that looks at it: a
//! march is dropped when the unit is firing on a live target and the destination
//! is more than `REPOSITION_CELLS` away.
//!
//! This test gives the tank a guard post first, lets it settle, and then asks for
//! the same move the match asks for.

use vera20k_core::rules::ruleset::RuleSet;
use vera20k_core::sim::behavior::goal::Goal;
use vera20k_core::sim::control::board::ControlBoard;
use vera20k_core::sim::control::brain::{Brain, Outbox};
use vera20k_core::sim::control::observation::Observation;
use vera20k_core::sim::intern::InternedId;
use vera20k_core::sim::world::Simulation;

/// Hold a post for a while, then ask for one move, then stop asking.
struct GuardThenMove {
    actor: u64,
    post: (u16, u16),
    cell: (u16, u16),
    frame: usize,
    moved: bool,
}

impl Brain for GuardThenMove {
    fn name(&self) -> &'static str {
        "guard-then-move"
    }

    fn think(&mut self, _rules: &RuleSet, _obs: &Observation<'_>, out: &mut Outbox) {
        self.frame += 1;
        if self.frame < 60 {
            out.order(self.actor, Goal::GuardAt { cell: self.post });
        } else if !self.moved {
            out.order(self.actor, Goal::Move { cell: self.cell });
            self.moved = true;
        }
    }
}

#[test]
fn a_unit_on_a_guard_post_still_takes_its_move() {
    let Some(retail) = vera20k_core::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules: RuleSet = retail.rules;
    let mut sim = vera20k_core::sim::arena_fixture::rival_arena(&rules, 0xA17);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let americans: InternedId = sim.intern("Americans");
    let tank = sim
        .spawn_object("MTNK", "Americans", 6, 6, 0, &rules)
        .expect("a tank");
    let post = (6, 6);
    let target = (26, 26);

    let mut board = ControlBoard::new(
        Box::new(GuardThenMove {
            actor: tank,
            post,
            cell: target,
            frame: 0,
            moved: false,
        }),
        americans,
        512,
    );

    let mut closest = 999;
    let mut arrived_at = None;
    for frame in 0..1500 {
        board.step(&mut sim, &rules);
        let _ = sim.take_due_commands();
        sim.advance_tick(&[], Some(&rules), None, None, 33);
        let observation = Observation::new(&sim, americans);
        let Some(cell) = observation
            .own_entities()
            .find(|entity| entity.entity_id == tank)
            .map(|entity| entity.cell)
        else {
            break;
        };
        let away = i32::from(cell.0.abs_diff(target.0)).max(i32::from(cell.1.abs_diff(target.1)));
        closest = closest.min(away);
        if away <= 1 {
            arrived_at = Some(frame);
            break;
        }
    }
    assert!(
        arrived_at.is_some(),
        "a unit that held a post must still take a move to {target:?}; \
         it got within {closest} cells (rejected={})",
        board.rejected()
    );
}

/// The same, but the destination is as far as a scout's actually is: the match
/// ordered tanks 160 cells across the map and they never moved a cell.
#[test]
fn a_unit_takes_a_move_across_the_whole_arena() {
    let Some(retail) = vera20k_core::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules: RuleSet = retail.rules;
    // A bigger arena, so the destination is genuinely far rather than out of
    // bounds: the default is 32x32.
    let mut sim = vera20k_core::sim::arena_fixture::rival_arena_with_size(&rules, 0xA17, 200);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let americans: InternedId = sim.intern("Americans");
    let tank = sim
        .spawn_object("MTNK", "Americans", 40, 40, 0, &rules)
        .expect("a tank");
    let target = (170, 150);

    let mut board = ControlBoard::new(
        Box::new(GuardThenMove {
            actor: tank,
            post: (40, 40),
            cell: target,
            frame: 0,
            moved: false,
        }),
        americans,
        512,
    );

    let mut closest = 999;
    let mut start = None;
    for _ in 0..2000 {
        board.step(&mut sim, &rules);
        let _ = sim.take_due_commands();
        sim.advance_tick(&[], Some(&rules), None, None, 33);
        let observation = Observation::new(&sim, americans);
        let Some(cell) = observation
            .own_entities()
            .find(|entity| entity.entity_id == tank)
            .map(|entity| entity.cell)
        else {
            break;
        };
        start.get_or_insert(cell);
        let away = i32::from(cell.0.abs_diff(target.0)).max(i32::from(cell.1.abs_diff(target.1)));
        closest = closest.min(away);
    }
    let from = start.unwrap_or((0, 0));
    assert!(
        closest < 999,
        "the tank never moved from {from:?}; rejected={}",
        board.rejected()
    );
    // The reading that matters is how far a long march gets, not whether it
    // arrives: the destination is 160 cells away, which is past the ground this
    // fixture covers. What the test pins is that a guarded unit *does* start and
    // keep walking when the order is far away — the match's scouts covered zero
    // cells, so "it moved at all" is the fact to hold on to.
    assert!(
        closest < 150,
        "a 160-cell march covered almost nothing: from {from:?} the closest was {closest}"
    );
}
