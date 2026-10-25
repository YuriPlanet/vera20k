//! The campaign carry-over record at `0x008A38E0`: what one campaign mission
//! hands to the next Start_Scenario, and what a restart or defeat retry
//! hands back to the same mission.
//!
//! Native owners (instruction-read):
//! - `0x004C6140` clears it at session preparation (`0x0052DF0E`): the 50
//!   globals false, money +0x34 and timer +0x38 zero, the player's
//!   difficulty row +0x3C = 1, the stage word +0x40 zero.
//! - `0x004C6180` saves it before a won mission's Start_Scenario: the 50
//!   Scenario globals (`0x004C6187..0x004C61A1`), the player's money and
//!   remaining mission timer, the player House's difficulty row House+0x184
//!   (`0x004C61E0..0x004C61EC`) and Scenario+0x35A0.
//! - `0x004C6210` writes it back after Start_Scenario has read the map, for
//!   the won mission (`0x00685D7A`), the in-battle restart (`0x00686506`) and
//!   the defeat retry (`0x0068639A`): each global through SetGlobal
//!   `0x00689670` in index order, then the money, then
//!   `HouseClass::SetDifficulty` `0x004F6EC0` on the player House with +0x3C
//!   (`0x004C628B..0x004C6295`), then the timer and Scenario+0x35A0.
//! - `ContentSave67D31B` / `ContentLoad67E804` call `4C6340` / `4C6320`
//!   to write/read all 0x44 bytes. ScenarioSession retains this incoming
//!   record separately from the current mission's changing global flags.
//!
//! Money uses the existing Available_Money owner's represented storage domain;
//! this does not add its separate, pre-existing nonempty-silo mechanism.

use crate::rules::ruleset::RuleSet;
use crate::sim::house_state::HouseDifficulty;
use crate::sim::trigger_runtime::SCENARIO_GLOBAL_COUNT;
use crate::sim::world::Simulation;
use crate::util::native_x87::{MaskedX87Chop53 as X87, MaskedX87Ordering};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, serde::Serialize, serde::Deserialize)]
pub struct CampaignCarryOver {
    #[serde(with = "saved_globals")]
    globals: [bool; SCENARIO_GLOBAL_COUNT],
    /// Record+0x3C, the player House's difficulty row.
    player_difficulty: HouseDifficulty,
    /// Record+0x38; absent in older VERA saves, which did not carry timers.
    #[serde(default)]
    countdown_frames: i32,
    /// Record+34: player IHouse+18 -> Available_Money4F6990.
    #[serde(default)]
    money: i32,
    /// Evade record+40, raw Scenario+35A0 stage word (Save4C61F5/Do4C630D).
    map_stage: u16,
}

mod saved_globals {
    use super::SCENARIO_GLOBAL_COUNT;
    use serde::de::{SeqAccess, Visitor};
    use serde::ser::SerializeTuple;

    pub fn serialize<S: serde::Serializer>(
        values: &[bool; SCENARIO_GLOBAL_COUNT],
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        let mut tuple = serializer.serialize_tuple(SCENARIO_GLOBAL_COUNT)?;
        for value in values {
            tuple.serialize_element(value)?;
        }
        tuple.end()
    }

    pub fn deserialize<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<[bool; SCENARIO_GLOBAL_COUNT], D::Error> {
        struct Globals;
        impl<'de> Visitor<'de> for Globals {
            type Value = [bool; SCENARIO_GLOBAL_COUNT];

            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("50 campaign globals")
            }

            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                let mut values = [false; SCENARIO_GLOBAL_COUNT];
                for (index, value) in values.iter_mut().enumerate() {
                    *value = seq
                        .next_element()?
                        .ok_or_else(|| serde::de::Error::invalid_length(index, &self))?;
                }
                Ok(values)
            }
        }
        deserializer.deserialize_tuple(SCENARIO_GLOBAL_COUNT, Globals)
    }
}

impl CampaignCarryOver {
    /// The record `0x004C6140` leaves: every global false and row 1.
    pub const CLEARED: Self = Self {
        globals: [false; SCENARIO_GLOBAL_COUNT],
        player_difficulty: HouseDifficulty::Normal,
        countdown_frames: 0,
        money: 0,
        map_stage: 0,
    };

    pub const fn globals(&self) -> &[bool; SCENARIO_GLOBAL_COUNT] {
        &self.globals
    }

    pub const fn player_difficulty(&self) -> HouseDifficulty {
        self.player_difficulty
    }

    pub const fn countdown_frames(&self) -> i32 {
        self.countdown_frames
    }

    pub const fn money(&self) -> i32 {
        self.money
    }

    pub const fn map_stage(&self) -> u16 {
        self.map_stage
    }
}

impl Simulation {
    /// `0x004C6180`: the record a won mission saves for the next one. A
    /// session without a player House keeps the cleared row.
    pub fn save_campaign_carry_over(&self) -> CampaignCarryOver {
        let player = self
            .session
            .current_house()
            .and_then(|player| self.houses.get(&player));
        let player_difficulty = player
            .map_or(CampaignCarryOver::CLEARED.player_difficulty, |house| {
                house.difficulty()
            });
        CampaignCarryOver {
            globals: self.global_variables(),
            player_difficulty,
            countdown_frames: self.session.campaign_countdown_frames(),
            money: player.map_or(0, |house| house.economy().available_money()),
            map_stage: self.session.campaign_map_stage(),
        }
    }

    /// `0x004C6210`: write the record back once Start_Scenario has read the
    /// map. SetDifficulty runs on the player House only; computer Houses keep
    /// the rows the scenario read gave them.
    pub fn restore_campaign_carry_over(&mut self, record: &CampaignCarryOver, rules: &RuleSet) {
        for (index, &set) in record.globals.iter().enumerate() {
            Self::set_trigger_variable(
                &self.session,
                &mut self.scenario_rng,
                &mut self.trigger_runtime,
                true,
                index as i32,
                set,
            );
        }
        let Some(player) = self.session.current_house() else {
            return;
        };
        let Some(array_index) = self.houses.index_of(player) else {
            return;
        };
        // EvadeClass::Do4C622F..4C6285: FILD saved Available_Money, multiply
        // the receiving Scenario's double, cap unless-1, ftol low32, then
        // wallet/baseline adds. Reuse the economy's deterministic masked
        // arithmetic owner (including signed truncation and overflow).
        let (multiplier, cap) = self.session.campaign_money_policy();
        let mut amount = X87::mul(X87::load_i32(record.money), X87::load_f64(multiplier));
        if cap != -1
            && matches!(
                X87::compare(amount, X87::load_i32(cap)),
                MaskedX87Ordering::Equal | MaskedX87Ordering::Greater
            )
        {
            amount = X87::load_i32(cap);
        }
        if let Some(house) = self.houses.get_mut(&player) {
            house
                .economy_mut()
                .add_campaign_carry_over(X87::ftol_i32_low_masked(amount));
        }
        let frame = self.session.binary_frame() as i32;
        let country = self
            .houses
            .get(&player)
            .and_then(|house| house.country())
            .map(|country| self.interner.resolve(country).to_owned());
        let biases = country.map_or_else(Default::default, |name| {
            rules.country_difficulty_biases(&name)
        });
        if let Some(house) = self.houses.get_mut(&player) {
            house.set_difficulty(
                record.player_difficulty,
                &rules.general,
                biases,
                self.session.game_mode_nonzero(),
                array_index as i32,
                frame,
            );
        }
        self.session
            .inherit_campaign_countdown(record.countdown_frames);
        self.session.set_campaign_map_stage(record.map_stage);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::scenario_session::{ScenarioDescriptor, ScenarioTimerAction};
    use crate::util::native_x87::NativeF64Bits;

    #[test]
    fn saved_stage_and_incoming_restart_stage_remain_distinct() {
        let rules = RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str("")).unwrap();
        let mut sim = timer_campaign(&crate::rules::ini_parser::IniFile::from_str(""));
        assert_eq!(sim.session().campaign_map_stage(), 0);
        sim.set_campaign_map_stage(u16::MAX);
        let incoming = sim.save_campaign_carry_over();
        sim.session.initialize_campaign_for_test(0, 2, incoming);
        sim.set_campaign_map_stage(1);
        let hash = sim.state_hash();
        let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "stage", 0);
        let mut loaded = crate::sim::snapshot::GameSnapshot::load(&bytes)
            .unwrap()
            .sim;
        assert_eq!(loaded.state_hash(), hash);
        assert_eq!(loaded.session().campaign_map_stage(), 1);
        let restart = loaded.session().campaign_restart_carry_over().unwrap();
        assert_eq!(restart.map_stage(), u16::MAX);
        loaded.restore_campaign_carry_over(&restart, &rules);
        assert_eq!(loaded.session().campaign_map_stage(), u16::MAX);
        assert_ne!(loaded.state_hash(), hash);
        loaded.restore_campaign_carry_over(&CampaignCarryOver::CLEARED, &rules);
        assert_eq!(loaded.session().campaign_map_stage(), 0);
    }

    fn timer_campaign(ini: &crate::rules::ini_parser::IniFile) -> Simulation {
        let basic = crate::map::basic::parse_basic_section(ini);
        let mut sim = Simulation::from_descriptor(&ScenarioDescriptor {
            timer_inherit: basic.timer_inherit.unwrap_or(false),
            carry_over_money: basic.carry_over_money,
            carry_over_cap: basic.carry_over_cap,
            ..Default::default()
        });
        let player = sim.interner.intern("Player");
        sim.houses.get_or_register_for_test(player, || {
            crate::sim::house_state::HouseState::new(player, 0, None, false, 0, 10)
        });
        sim.session.set_current_house_for_test(Some(player));
        sim
    }

    #[test]
    fn receiving_money_cap_survives_saved_campaign_restart() {
        let rules = RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str("")).unwrap();
        let mut source = timer_campaign(&crate::rules::ini_parser::IniFile::from_str(""));
        let player = source.session.current_house().unwrap();
        source
            .houses
            .get_mut(&player)
            .unwrap()
            .economy_mut()
            .add_credits(10_001);
        let incoming = source.save_campaign_carry_over();
        let mut next = timer_campaign(&crate::rules::ini_parser::IniFile::from_str(
            "[Basic]\nCarryOverMoney=25%\nCarryOverCap=2000\n",
        ));
        let player = next.session.current_house().unwrap();
        next.houses
            .get_mut(&player)
            .unwrap()
            .economy_mut()
            .initialize_scenario_credits(70, None, false, 0, 0);
        next.session.initialize_campaign_for_test(3, 5, incoming);
        let bytes = crate::sim::snapshot::GameSnapshot::save(&next, 0, 0, "", 0);
        let mut restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
            .unwrap()
            .sim;
        let incoming = restored.session.campaign_restart_carry_over().unwrap();
        restored.restore_campaign_carry_over(&incoming, &rules);
        let economy = restored.houses.get(&player).unwrap().economy();
        assert_eq!(economy.credits(), 9000);
        assert_eq!(economy.scenario_credits(), 9000);
        assert_eq!(economy.score(), 0);
        assert_eq!(economy.spent_credits(), 0);
    }

    #[test]
    fn money_policy_uses_destination_defaults_and_signed_uncapped_amounts() {
        let rules = RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str("")).unwrap();
        let mut source = timer_campaign(&crate::rules::ini_parser::IniFile::from_str(""));
        let player = source.session.current_house().unwrap();
        source
            .houses
            .get_mut(&player)
            .unwrap()
            .economy_mut()
            .add_credits(10_001);
        let incoming = source.save_campaign_carry_over();
        let mut upper = timer_campaign(&crate::rules::ini_parser::IniFile::from_str(
            "[Basic]\nCarryOverMoney=200%\nCarryOverCap=-1\n",
        ));
        upper.restore_campaign_carry_over(&incoming, &rules);
        assert_eq!(upper.houses[&player].economy().credits(), 10_001);
        let mut default_cap = timer_campaign(&crate::rules::ini_parser::IniFile::from_str(
            "[Basic]\nCarryOverMoney=25%\n",
        ));
        default_cap.restore_campaign_carry_over(&incoming, &rules);
        assert_eq!(default_cap.houses[&player].economy().credits(), 0);
        let mut negative = timer_campaign(&crate::rules::ini_parser::IniFile::from_str(
            "[Basic]\nCarryOverMoney=-25%\nCarryOverCap=-1\n",
        ));
        negative.restore_campaign_carry_over(&incoming, &rules);
        assert_eq!(negative.houses[&player].economy().credits(), -2500);
        assert_eq!(negative.houses[&player].economy().scenario_credits(), -2500);
    }

    #[test]
    fn stock_sov02_keeps_its_money_when_carry_over_is_disabled() {
        let Some(bytes) = crate::rules::retail_ini_fixture::retail_archive_bytes("SOV02SMD.MAP")
        else {
            return;
        };
        let map = crate::map::map_file::MapFile::from_bytes(&bytes).unwrap();
        assert_eq!(
            map.basic.carry_over_money,
            Some(NativeF64Bits::POSITIVE_ZERO)
        );
        assert_eq!(map.basic.carry_over_cap, Some(0));
        let rules = RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str("")).unwrap();
        let mut next = timer_campaign(&map.ini);
        let player = next.session.current_house().unwrap();
        let record = CampaignCarryOver {
            money: 10_001,
            ..CampaignCarryOver::CLEARED
        };
        next.restore_campaign_carry_over(&record, &rules);
        assert_eq!(next.houses[&player].economy().credits(), 0);
    }

    #[test]
    fn incoming_countdown_survives_save_load_and_resumes_paused_time() {
        let rules =
            RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str("")).expect("rules");
        let mut source = timer_campaign(&crate::rules::ini_parser::IniFile::from_str(""));
        source.session.set_binary_frame_for_test(123);
        source
            .session
            .change_scenario_timer(ScenarioTimerAction::Set(120));
        source.session.set_binary_frame_for_test(423);
        source
            .session
            .change_scenario_timer(ScenarioTimerAction::Stop);
        let incoming = source.save_campaign_carry_over();

        let mut next = timer_campaign(&crate::rules::ini_parser::IniFile::from_str(
            "[Basic]\nTimerInherit=yes\n",
        ));
        next.session.initialize_campaign_for_test(3, 5, incoming);
        next.session
            .change_scenario_timer(ScenarioTimerAction::Set(5));
        next.session.set_scenario_timer_caption("DESC:NextMap");
        let bytes = crate::sim::snapshot::GameSnapshot::save(&next, 0, 0, "", 0);
        let mut restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
            .unwrap()
            .sim;
        restored.session.set_binary_frame_for_test(5000);
        let record = restored.session.campaign_restart_carry_over().unwrap();
        restored.restore_campaign_carry_over(&record, &rules);
        assert_eq!(
            restored.session.scenario_timer_view(),
            Some((1500, "DESC:NextMap"))
        );
        restored.session.set_binary_frame_for_test(6500);
        assert!(restored.session.scenario_timer_elapsed());
        restored.session.finish_scenario_timer_poll();
        assert_eq!(restored.session.scenario_timer_view(), None);
    }

    #[test]
    fn stock_opt_out_and_nonpositive_records_keep_the_receiving_timer() {
        let Some(bytes) = crate::rules::retail_ini_fixture::retail_archive_bytes("SOV02SMD.MAP")
        else {
            return;
        };
        let map = crate::map::map_file::MapFile::from_bytes(&bytes).unwrap();
        assert_eq!(map.basic.timer_inherit, Some(false));
        let rules = RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str("")).unwrap();
        let mut receiving = timer_campaign(&map.ini);
        receiving
            .session
            .change_scenario_timer(ScenarioTimerAction::Set(30));
        let rng = receiving.rng_state();
        let positive = receiving.save_campaign_carry_over();
        receiving
            .session
            .change_scenario_timer(ScenarioTimerAction::Set(5));
        receiving.restore_campaign_carry_over(&positive, &rules);
        assert_eq!(receiving.session.scenario_timer_view(), Some((75, "")));
        assert_eq!(receiving.rng_state(), rng);

        let mut opted_in = timer_campaign(&crate::rules::ini_parser::IniFile::from_str(
            "[Basic]\nTimerInherit=yes\n",
        ));
        opted_in
            .session
            .change_scenario_timer(ScenarioTimerAction::Set(30));
        opted_in
            .session
            .set_scenario_timer_caption("DESC:Receiving");
        opted_in.restore_campaign_carry_over(&CampaignCarryOver::CLEARED, &rules);
        assert_eq!(
            opted_in.session.scenario_timer_view(),
            Some((450, "DESC:Receiving"))
        );
        receiving
            .session
            .change_scenario_timer(ScenarioTimerAction::Set(-1));
        receiving
            .session
            .change_scenario_timer(ScenarioTimerAction::Stop);
        opted_in.restore_campaign_carry_over(&receiving.save_campaign_carry_over(), &rules);
        assert_eq!(
            opted_in.session.scenario_timer_view(),
            Some((450, "DESC:Receiving"))
        );
    }

    /// Restarting the first mission restores the cleared record, whose row 1
    /// replaces the menu row on the player House alone.
    #[test]
    fn restore_sets_the_player_row_and_globals_only() {
        let rules =
            RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str("")).expect("rules");
        let mut sim = Simulation::new();
        let player = sim.interner.intern("Player");
        let enemy = sim.interner.intern("Enemy");
        for (name, row) in [
            (player, HouseDifficulty::Easy),
            (enemy, HouseDifficulty::Hard),
        ] {
            let house = sim.houses.get_or_register_for_test(name, || {
                crate::sim::house_state::HouseState::new(name, 0, None, false, 0, 10)
            });
            house.set_difficulty(row, &rules.general, Default::default(), false, 0, 0);
        }
        sim.session.set_current_house_for_test(Some(player));
        let mut globals = [false; SCENARIO_GLOBAL_COUNT];
        globals[7] = true;
        let record = CampaignCarryOver {
            globals,
            player_difficulty: HouseDifficulty::Hard,
            ..CampaignCarryOver::CLEARED
        };

        sim.restore_campaign_carry_over(&CampaignCarryOver::CLEARED, &rules);
        assert_eq!(
            sim.houses.get(&player).unwrap().difficulty(),
            HouseDifficulty::Normal
        );
        assert_eq!(
            sim.houses.get(&enemy).unwrap().difficulty(),
            HouseDifficulty::Hard
        );
        assert_eq!(sim.global_variables(), [false; SCENARIO_GLOBAL_COUNT]);

        sim.restore_campaign_carry_over(&record, &rules);
        assert_eq!(sim.save_campaign_carry_over(), record);
    }

    #[test]
    fn snapshot_preserves_incoming_carry_over_separately_from_current_globals() {
        let mut globals = [false; SCENARIO_GLOBAL_COUNT];
        globals[7] = true;
        globals[49] = true;
        let incoming = CampaignCarryOver {
            globals,
            player_difficulty: HouseDifficulty::Hard,
            ..CampaignCarryOver::CLEARED
        };
        let mut sim = Simulation::new();
        sim.session.initialize_campaign_for_test(3, 5, incoming);
        let rules =
            RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str("")).expect("rules");
        sim.restore_campaign_carry_over(&incoming, &rules);
        Simulation::set_trigger_variable(
            &sim.session,
            &mut sim.scenario_rng,
            &mut sim.trigger_runtime,
            true,
            7,
            false,
        );
        let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "", 0);
        let loaded = crate::sim::snapshot::GameSnapshot::load(&bytes)
            .unwrap()
            .sim;
        assert_eq!(loaded.session.campaign_index(), Some(3));
        assert_eq!(loaded.session.campaign_mission_counter(), Some(5));
        assert_eq!(loaded.session.campaign_restart_carry_over(), Some(incoming));
        assert!(!loaded.global_variables()[7]);
        assert!(loaded.global_variables()[49]);
        // Native ScenarioLoad resets the embedded RNG; whole-world hashes
        // across that boundary need not match. The retained gameplay record
        // and the current globals above are separate persistence assertions.
    }
}
