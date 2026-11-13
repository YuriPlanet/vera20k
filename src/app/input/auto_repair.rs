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
//! until its order has surely executed
//! ([`command_executed_by`](crate::app::input::commands::command_executed_by))
//! before it can get another.
//!
//! An automatic start is silent on this game: the "Repairing" line and the
//! click `ToggleRepair` plays for a wrench click are skipped
//! ([`QuietSounds`](crate::app::match_audio::QuietSounds)). The building's
//! flash is simulation state and stays, and other players' games, which do
//! not know the order was automatic, hear the click as they would a wrench
//! click.

use crate::app::AppState;
use crate::app::input::commands::{
    command_executed_by, preferred_local_owner_name, try_schedule_command,
};
use crate::app::match_audio::QuietSound;
use crate::rules::ruleset::RuleSet;
use crate::sim::command::Command;
use crate::sim::world::Simulation;

/// Game frames between two looks at the player's buildings.
const CHECK_INTERVAL: u64 = 15;
/// Orders one look may send; the rest follow at the next look.
const MAX_ORDERS: usize = 16;

/// The Repair orders sent whose buildings are still held back.
#[derive(Debug, Default)]
pub(crate) struct AutoRepairState {
    /// Building and the last frame its order holds it back.
    sent: Vec<(u64, u64)>,
}

impl AutoRepairState {
    /// Of the buildings that want repair, the ones to send the order now:
    /// those no earlier order holds back, at most [`MAX_ORDERS`]. Each is
    /// held back until frame `until`.
    pub(crate) fn orders(&mut self, wanting: &[u64], now: u64, until: u64) -> Vec<u64> {
        self.sent.retain(|&(_, last)| now < last);
        let mut orders = Vec::new();
        for &building in wanting {
            if orders.len() == MAX_ORDERS {
                break;
            }
            if self.sent.iter().all(|&(sent, _)| sent != building) {
                self.sent.push((building, until));
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

/// The sounds a Repair order that starts `building`'s repair makes on this
/// game (`toggle_repair`, `BuildingClass::ToggleRepair 0x00446FF0`): the
/// "Repairing" line, then `GenericClick=` at the building.
fn start_sounds(sim: &Simulation, rules: &RuleSet, building: u64) -> Vec<QuietSound> {
    let Some(entity) = sim.entities().get(building) else {
        return Vec::new();
    };
    let mut sounds = vec![QuietSound::Repairing {
        owner: entity.owner(),
        type_id: entity.type_ref(),
    }];
    if let Some(sound_id) = rules.general.generic_click_sound.clone() {
        sounds.push(QuietSound::VocAt {
            sound_id,
            rx: entity.position.rx(),
            ry: entity.position.ry(),
            sub_x: entity.position.sub_x(),
            sub_y: entity.position.sub_y(),
        });
    }
    sounds
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
    let (now, until, orders) = {
        let Some(runtime) = state.match_state.sim_runtime.runtime() else {
            return;
        };
        let sim = &runtime.simulation;
        let rules = &runtime.resources.rules;
        let now = sim.session().tick();
        if now % CHECK_INTERVAL != 0 {
            return;
        }
        let Some(owner) = sim.interner().get(&owner_name) else {
            return;
        };
        let until = command_executed_by(state, now);
        let wanting = crate::sim::production::buildings_wanting_repair(sim, rules, owner);
        let orders: Vec<_> = state
            .match_state
            .input
            .auto_repair
            .orders(&wanting, now, until)
            .into_iter()
            .map(|building| (building, start_sounds(sim, rules, building)))
            .collect();
        (now, until, orders)
    };
    for (entity_id, sounds) in orders {
        if try_schedule_command(state, &owner_name, Command::ToggleRepair { entity_id }).is_none() {
            state.match_state.input.auto_repair.not_sent(entity_id);
            continue;
        }
        // The order lands while it holds the building back, or not at all.
        for sound in sounds {
            state
                .match_state
                .match_audio
                .quiet
                .expect(sound, now, until);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_building_gets_one_order_until_the_wait_is_over() {
        let mut state = AutoRepairState::default();
        assert_eq!(state.orders(&[7, 9], 0, 90), [7, 9]);
        // Still not repairing at the next looks: the first order may not
        // have landed, and a second would switch the repair off again.
        assert_eq!(state.orders(&[7, 9, 11], 15, 105), [11]);
        assert_eq!(state.orders(&[7], 89, 179), [0u64; 0]);
        assert_eq!(state.orders(&[7], 90, 180), [7]);
    }

    #[test]
    fn one_look_sends_a_bounded_number_and_the_rest_follow() {
        let mut state = AutoRepairState::default();
        let wanting: Vec<u64> = (1..=MAX_ORDERS as u64 + 3).collect();
        assert_eq!(state.orders(&wanting, 0, 90).len(), MAX_ORDERS);
        assert_eq!(
            state.orders(&wanting, 15, 105),
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
        assert_eq!(state.orders(&[7], 0, 90), [7]);
        state.not_sent(7);
        assert_eq!(state.orders(&[7], 15, 105), [7]);
    }
}
