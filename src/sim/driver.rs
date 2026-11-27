//! Optional economy settings for an externally controlled computer house.

use crate::sim::economy::Economy;
use crate::util::native_x87::NativeF32Bits;
use serde::{Deserialize, Serialize};

/// A credit bonus paid every `interval_ticks` simulation frames after frame zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CreditDrip {
    pub amount: i32,
    pub interval_ticks: u32,
}

/// The simulation applies these settings to the house seated by an external driver.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default)]
pub struct DriverPolicy {
    /// Take the Hard row of MultiplayerAICM at Post_Map_Init (0x00686A52).
    pub native_hard_grant: bool,
    pub credit_drip: Option<CreditDrip>,
    /// Multiply the country's build time by this binary32 scale before the stock calculation.
    pub build_time_scale: Option<NativeF32Bits>,
    /// Top up available money every frame. This takes precedence over a drip.
    pub wallet_floor: Option<i32>,
}

impl DriverPolicy {
    /// Hand over decisions with no economic support.
    pub const fn neutral() -> Self {
        Self {
            native_hard_grant: false,
            credit_drip: None,
            build_time_scale: None,
            wallet_floor: None,
        }
    }

    pub(super) fn apply_credit_support(&self, economy: &mut Economy, frame: u32) {
        if let Some(floor) = self.wallet_floor {
            let available = economy.available_money();
            if available < floor {
                // Add_Credits adds modulo 2^32, including a top-up across zero.
                economy.add_credits(floor.wrapping_sub(available));
            }
        } else if let Some(drip) = self.credit_drip
            && frame != 0
            && drip.interval_ticks != 0
            && frame.is_multiple_of(drip.interval_ticks)
        {
            economy.add_credits(drip.amount);
        }
    }
}

#[cfg(all(test, tests_sim_c_g))]
mod tests {
    use super::*;
    use crate::sim::house_state::HouseState;
    use crate::sim::scenario_bootstrap::{
        apply_skirmish_ai_opening_credits, normalized_launch_slots, populate_launch_houses,
    };
    use crate::sim::snapshot::GameSnapshot;
    use crate::sim::world::Simulation;
    use crate::skirmish_launch::{AiDifficulty, LaunchCountry};

    #[test]
    fn easy_driver_gets_the_hard_grant_on_the_post_starting_units_wallet() {
        let Some(retail) =
            crate::rules::retail_ini_fixture::retail_battle_rules_for_map("Dustbowl.mmx")
        else {
            return;
        };
        let mut launch = crate::headless_scenario::battle_session(
            "Dustbowl.mmx",
            &[(LaunchCountry::Russia, AiDifficulty::Easy)],
        );
        let policy = DriverPolicy {
            native_hard_grant: true,
            ..DriverPolicy::neutral()
        };
        let slot = &mut launch.opponents[0];
        assert!(
            serde_json::to_value(&*slot)
                .unwrap()
                .get("driver_policy")
                .is_none()
        );
        slot.driver_policy = Some(policy);
        let mut sim = Simulation::with_seed(1);
        let slots = normalized_launch_slots(&launch);
        populate_launch_houses(&mut sim, &slots, &retail.rules);
        let owner = sim.interner.get("Computer1").unwrap();
        let house = sim.houses.get_mut(&owner).unwrap();
        assert_eq!(house.driver_policy(), Some(&policy));
        // Starting units can return unused budget between construction and Post_Map_Init.
        house.economy_mut().add_credits(100);
        let balance = house.economy().available_money();
        apply_skirmish_ai_opening_credits(&mut sim.houses, &retail.rules);
        assert_eq!(sim.houses[&owner].economy().available_money(), balance * 5);
    }

    #[test]
    fn credit_support_runs_once_per_frame_and_survives_save_load() {
        let mut sim = Simulation::with_seed(1);
        let drip = CreditDrip {
            amount: 10,
            interval_ticks: 2,
        };
        let mut register = |name, floor| {
            let owner = sim.interner.intern(name);
            let mut house = HouseState::new(owner, 0, None, false, -1, 10);
            house.make_command_driven(DriverPolicy {
                credit_drip: Some(drip),
                wallet_floor: floor,
                ..DriverPolicy::neutral()
            });
            sim.houses.register(house);
            owner
        };
        let floored = register("Computer1", Some(i32::MAX));
        let dripping = register("Computer2", None);
        sim.advance_tick(&[], None, None, None, 67);
        assert_eq!(sim.houses[&floored].economy().credits(), i32::MAX);
        assert_eq!(sim.houses[&dripping].economy().credits(), -1);
        let bytes = GameSnapshot::save(&sim, 0, 0, "driver", 0);
        let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
        assert_eq!(
            restored.houses[&floored].driver_policy(),
            sim.houses[&floored].driver_policy()
        );
        restored.advance_tick(&[], None, None, None, 67);
        restored.advance_tick(&[], None, None, None, 67);
        assert_eq!(restored.houses[&floored].economy().credits(), i32::MAX);
        assert_eq!(restored.houses[&dripping].economy().credits(), 9);
    }

    #[test]
    fn driven_computer_executes_orders_and_leaves_its_strategy_timer_alone() {
        use crate::sim::command::{Command, CommandEnvelope};

        let mut sim = Simulation::with_seed(1);
        let owner = sim.interner.intern("Computer1");
        let mut house = HouseState::new(owner, 0, None, false, 0, 10);
        house.make_command_driven(DriverPolicy::neutral());
        let timer = *house.strategy_timer();
        sim.houses.register(house);
        let command = CommandEnvelope::new(owner, 1, Command::LiveGameSpeed { speed: 4 });
        let result = sim.advance_tick(&[command], None, None, None, 67);
        assert_eq!(result.executed_commands, 1);
        assert_eq!(sim.session.game_options().game_speed, 4);
        assert_eq!(*sim.houses[&owner].strategy_timer(), timer);
    }
}
