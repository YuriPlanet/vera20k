//! Does the policy's own move goal survive the order door?
//!
//! In a match a unit given `Goal::Move` by the policy does not travel, while the
//! same goal through `Funnel::prepare` in `move_tests.rs` carries a tank twenty
//! cells. The door has one rule that reports success while dropping the order —
//! `order_stands` answers `Queued` for a destination the unit already holds — so
//! the first question is whether the policy's order ever reaches the funnel at
//! all. This drives the real `ControlBoard`, the object the match uses, rather
//! than `Funnel` alone.

use vera20k_core::rules::ruleset::RuleSet;
use vera20k_core::sim::behavior::goal::Goal;
use vera20k_core::sim::control::board::ControlBoard;
use vera20k_core::sim::control::brain::{Brain, Outbox};
use vera20k_core::sim::control::observation::Observation;
use vera20k_core::sim::intern::InternedId;
use vera20k_core::sim::world::Simulation;

/// A brain that asks for exactly one move, on the first frame it is asked, and
/// nothing else ever. Everything after that is the door's decision.
struct OneMove {
    actor: u64,
    cell: (u16, u16),
    asked: bool,
}

impl Brain for OneMove {
    fn name(&self) -> &'static str {
        "one-move"
    }

    fn think(&mut self, _rules: &RuleSet, _obs: &Observation<'_>, out: &mut Outbox) {
        if !self.asked {
            out.order(self.actor, Goal::Move { cell: self.cell });
            self.asked = true;
        }
    }
}

#[test]
fn the_board_carries_one_move_to_its_destination() {
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
    let target = (26, 26);

    let mut board = ControlBoard::new(
        Box::new(OneMove {
            actor: tank,
            cell: target,
            asked: false,
        }),
        americans,
        512,
    );

    let mut closest = 999;
    for _ in 0..1200 {
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
            break;
        }
    }
    assert_eq!(
        board.rejected(),
        0,
        "the door must not refuse the move: closest was {closest} cells"
    );
    assert!(
        closest <= 1,
        "one Move through the real control board must carry the tank to {target:?}; \
         closest was {closest} cells"
    );
}
