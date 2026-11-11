//! VERA Auto Repair: with the Options page's Auto Repair box ticked, the
//! player's damaged buildings are sent the Repair order by themselves. See
//! `docs/auto-repair.md`.
//!
//! A helper on the player's own game, like AutoLoad and the ferry: it emits
//! only the ordinary REPAIR event, the one a wrench click on the building
//! sends (`DisplayClass` BandBox_LeftUp `0x004AC0E7`, executed by
//! `EventClass::Execute 0x004C6EFD` through `BuildingClass::ToggleRepair
//! 0x00446FF0`). Nothing in the simulation or on the network changes, and
//! other players' games need no knowledge of it. Which buildings want the
//! order is the simulation's own answer
//! ([`buildings_wanting_repair`](crate::sim::production::buildings_wanting_repair)).
//!
//! The REPAIR event is a toggle, so a second one sent before the first has
//! landed would switch the repair off again. Each building therefore waits
//! [`RESEND_TICKS`] after an order before it can get another.

use crate::app::AppState;
use crate::app::input::commands::{preferred_local_owner_name, try_schedule_command};
use crate::sim::command::Command;

/// Game frames between two looks at the player's buildings.
const CHECK_INTERVAL: u64 = 15;
/// Frames a building waits after an order before it can get another: longer
/// than an order takes to land online, and a limit on how often a repair that
/// keeps stopping for lack of money is started again.
const RESEND_TICKS: u64 = 90;
/// Orders one look may send; the rest follow at the next look.
const MAX_ORDERS: usize = 16;

/// The Repair orders sent and not yet old enough to send again.
#[derive(Debug, Default)]
pub(crate) struct AutoRepairState {
    /// Building and the frame its order was sent.
    sent: Vec<(u64, u64)>,
}

impl AutoRepairState {
    /// Of the buildings that want repair, the ones to send the order now:
    /// those without an order in the last [`RESEND_TICKS`] frames, at most
    /// [`MAX_ORDERS`]. Each is noted as sent at `now`.
    pub(crate) fn orders(&mut self, wanting: &[u64], now: u64) -> Vec<u64> {
        self.sent
            .retain(|&(_, at)| now.saturating_sub(at) < RESEND_TICKS);
        let mut orders = Vec::new();
        for &building in wanting {
            if orders.len() == MAX_ORDERS {
                break;
            }
            if self.sent.iter().all(|&(sent, _)| sent != building) {
                self.sent.push((building, now));
                orders.push(building);
            }
        }
        orders
    }

    /// Forget an order the command queue did not take, so the next look
    /// sends it again.
    fn not_sent(&mut self, building: u64) {
        self.sent.retain(|&(sent, _)| sent != building);
    }
}

/// Once per logic frame: every [`CHECK_INTERVAL`] frames, while the option is
/// on, send the Repair order to the local player's buildings that want it.
pub(crate) fn frame_pass(state: &mut AppState) {
    if !state.persistence.options_profile.auto_repair {
        state.match_state.input.auto_repair.sent.clear();
        return;
    }
    let Some(owner_name) = preferred_local_owner_name(state) else {
        return;
    };
    let orders = {
        let Some(runtime) = state.match_state.sim_runtime.runtime() else {
            return;
        };
        let sim = &runtime.simulation;
        let now = sim.session().tick();
        if now % CHECK_INTERVAL != 0 {
            return;
        }
        let Some(owner) = sim.interner().get(&owner_name) else {
            return;
        };
        let wanting =
            crate::sim::production::buildings_wanting_repair(sim, &runtime.resources.rules, owner);
        state.match_state.input.auto_repair.orders(&wanting, now)
    };
    for entity_id in orders {
        if try_schedule_command(state, &owner_name, Command::ToggleRepair { entity_id }).is_none() {
            state.match_state.input.auto_repair.not_sent(entity_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_building_gets_one_order_until_the_resend_wait_is_over() {
        let mut state = AutoRepairState::default();
        assert_eq!(state.orders(&[7, 9], 0), [7, 9]);
        // Still not repairing at the next looks: the first order may not
        // have landed, and a second would switch the repair off again.
        assert_eq!(state.orders(&[7, 9, 11], 15), [11]);
        assert_eq!(state.orders(&[7], RESEND_TICKS - 1), [0u64; 0]);
        assert_eq!(state.orders(&[7], RESEND_TICKS), [7]);
    }

    #[test]
    fn one_look_sends_a_bounded_number_and_the_rest_follow() {
        let mut state = AutoRepairState::default();
        let wanting: Vec<u64> = (1..=MAX_ORDERS as u64 + 3).collect();
        assert_eq!(state.orders(&wanting, 0).len(), MAX_ORDERS);
        assert_eq!(
            state.orders(&wanting, 15),
            [
                MAX_ORDERS as u64 + 1,
                MAX_ORDERS as u64 + 2,
                MAX_ORDERS as u64 + 3
            ]
        );
    }

    #[test]
    fn an_order_the_queue_refused_is_sent_again_at_the_next_look() {
        let mut state = AutoRepairState::default();
        assert_eq!(state.orders(&[7], 0), [7]);
        state.not_sent(7);
        assert_eq!(state.orders(&[7], 15), [7]);
    }
}
