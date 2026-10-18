//! The house's wallet, owned by `HouseState`: its cash balance (House
//! `+0x30C`), the spending statistic (`+0x2DC`) and the score (`+0x54E8`).
//! Every writer goes through the money primitives here, each the one port of
//! its native function.
//!
//! The house's silo aggregate (`+0x2FC`) is not represented here. Spend_Money
//! omits its silo-drain arm; Available_Money omits its storage-value term
//! and assumes a finite native HouseType `IncomeMult`. The corresponding
//! tools/house_money_oracle.py controls supply an empty aggregate.
//! `Added_To_Game @ 0x00502B83` merges a building's store (`+0x33C`), while
//! the `Weeder=` deposit (`0x004F9700`, called at `0x0073E48E`) writes the
//! separate store `+0x314`. Raw native Save/Load preserves these slots, so
//! the empty-store boundary must not be extended to arbitrary native saves.
//!
//! `Fill_In_Data`'s exact `[Basic] FillSilos=` path (`0x00684F02..0x00684F7C`)
//! is a fresh-start cash/score writer, not a storage fill: it retains
//! the initial Available_Money as a local budget. While that budget exceeds
//! the first Tiberium's Value and Available_Storage is positive, it calls
//! Add_Tiberium_Credits with amount1.0/type0 and subtracts Value from the
//! local budget. The credit callee writes no storage.
//! Native `Set_Defaults @ 0x00683893` clears the flag. It is absent in the
//! inspected stock multiplayer map/mode corpus; custom true remains accepted.
//!
//! Never depends on render/ui/audio/net (sim invariant #1). The wallet is
//! serialized and hashed.

use crate::rules::ruleset::INCOME_PPM_SCALE;

/// Per-house wallet and statistics. There is no second house credit balance.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Economy {
    /// House `+0x30C`, the cash balance.
    credits: i32,
    /// House+1DC: the map's Credits read, scaled by100 before the campaign
    /// money adjustment. This baseline remains separate from the live wallet.
    #[serde(default)]
    scenario_credits: i32,
    /// House `+0x2DC`: the running total `Spend_Money` took.
    spent_credits: i32,
    /// House `+0x54E8`, the score: refinery deposits, kills and captures add
    /// to it, and the score screen reads it.
    score: i32,
}

impl Economy {
    /// A wallet holding `credits`: `HouseClass::Set_Credits_And_Color @
    /// 0x004FCE00` seeds the balance of a new house.
    pub(crate) const fn new(credits: i32) -> Self {
        Self {
            credits,
            scenario_credits: 0,
            spent_credits: 0,
            score: 0,
        }
    }

    pub const fn credits(&self) -> i32 {
        self.credits
    }

    pub const fn spent_credits(&self) -> i32 {
        self.spent_credits
    }

    pub const fn score(&self) -> i32 {
        self.score
    }

    /// House ReadScenarioINI500B40: wrapping Credits*100 sets +1DC and the
    /// wallet. Only campaign PlayerControl houses apply the option0/2 delta,
    /// then clamp signed values <=0. Later Basic.Player forcing does not rerun
    /// this reader. Executed controls: input_oracle/campaign_start --houses.
    pub(crate) fn initialize_scenario_credits(
        &mut self,
        raw_credits: i32,
        campaign_difficulty: Option<crate::sim::house_state::HouseDifficulty>,
        player_control: bool,
        money_delta_easy: i32,
        money_delta_hard: i32,
    ) {
        self.scenario_credits = raw_credits.wrapping_mul(100);
        self.credits = self.scenario_credits;
        if let Some(difficulty) = campaign_difficulty.filter(|_| player_control) {
            self.credits = match difficulty as i32 {
                0 => self.credits.wrapping_add(money_delta_easy),
                2 => self.credits.wrapping_add(money_delta_hard),
                _ => self.credits,
            }
            .max(0);
        }
    }

    pub const fn scenario_credits(&self) -> i32 {
        self.scenario_credits
    }

    /// `HouseClass::Available_Money @ 0x004F6990`: `ftol(storage value *
    /// IncomeMult + balance)`. For empty storage and finite native IncomeMult,
    /// this is exactly the retained signed balance for every i32 balance.
    /// Nonempty/native-save and nonfinite-income inputs remain outside this
    /// represented domain; the two consumed oracle controls use empty storage.
    pub const fn available_money(&self) -> i32 {
        self.credits
    }

    /// Fresh empty-store arm of FillInData684F2A..684F6B. Source/controls:
    /// tools/house_money_oracle.py, original binary pinned by its sidecar.
    /// The local budget is sampled once; AddTib changes cash/score only.
    /// Capacity is HouseTracking's retained+310, never a Building scan.
    ///
    /// Fresh House/Building6C95E0 zeroes amount stores; Added502B83 merges
    /// those zero Building amounts. Team Full fills Foot cargo, while ordinary
    /// unload4F9610 pays cash (Weeder uses separate+314). Thus capacity alone
    /// decides AvailableStorage on this fresh history. Native saves/nonempty
    /// stores must not enter here; this is not a general4F69D0 port.
    pub(crate) fn fill_silos_fresh_empty_store(
        &mut self,
        value: i32,
        capacity: i32,
        income_ppm: i64,
    ) {
        let mut budget = self.available_money();
        // Preserve signed comparison and SUB32, including custom zero/negative
        // Values. A division shortcut or safety clamp changes native behavior.
        while budget > value && capacity > 0 {
            self.add_tiberium_credits(value, 1, INCOME_PPM_SCALE, income_ppm);
            budget = budget.wrapping_sub(value);
        }
    }

    /// `HouseClass::Add_Credits @ 0x004F9950..0x004F9962`: the complete body
    /// adds the signed argument to House+30C modulo2^32 and writes only that
    /// balance. No score/spending, timer, RNG, detach or callback side effect.
    pub fn add_credits(&mut self, amount: i32) {
        self.credits = self.credits.wrapping_add(amount);
    }

    /// `HouseClass::Spend_Money @ 0x004F9790`. An amount the balance covers
    /// (signed `<=`) comes off it; otherwise the whole balance, even a
    /// negative one, is taken and zeroed, and the rest would drain the silos
    /// (`0x004F97DD..`, empty here). What was taken joins `+0x2DC`.
    pub(crate) fn spend_money(&mut self, amount: i32) {
        let taken = if amount <= self.credits {
            self.credits = self.credits.wrapping_sub(amount);
            amount
        } else {
            std::mem::take(&mut self.credits)
        };
        self.spent_credits = self.spent_credits.wrapping_add(taken);
    }

    /// The score's kill and capture awards: `TechnoClass::RecordKill @
    /// 0x0070300F` and `TechnoClass::ChangeOwner @ 0x007015D0` add to
    /// `+0x54E8`, wrapping.
    pub(crate) fn add_score(&mut self, points: i32) {
        self.score = self.score.wrapping_add(points);
    }

    /// `HouseClass::Add_Tiberium_Credits @ 0x004F9610` for `amount` units
    /// of a tiberium: `score = ftol(amount * 5 + score)` and `balance =
    /// ftol(value * IncomeMult * amount + balance)`, `value` being
    /// the tiberium's `Value=`. VERA's amount is `bales * scale_ppm / 1e6`
    /// (the deposit passes 1.0, its purifier bonus `P * PurifierBonus`), its
    /// `value` the bales' summed worth and the IncomeMult `income_ppm / 1e6`;
    /// each sum truncates toward zero once and keeps its low 32 bits, as the
    /// `ftol @ 0x007C5F00` does. Native multiplies floats: an IncomeMult or
    /// PurifierBonus a float cannot hold, such as 0.9, can pay a credit
    /// less there (the oracle's `IncomeMult 0.9` row); retail's
    /// PurifierBonus .25 and IncomeMult values a float holds pay the same.
    pub(crate) fn add_tiberium_credits(
        &mut self,
        value: i32,
        bales: i32,
        scale_ppm: i64,
        income_ppm: i64,
    ) {
        let scale = i128::from(INCOME_PPM_SCALE);
        let score = (i128::from(self.score) * scale
            + i128::from(bales) * i128::from(scale_ppm) * 5)
            / scale;
        let credits = (i128::from(self.credits) * scale * scale
            + i128::from(value) * i128::from(scale_ppm) * i128::from(income_ppm))
            / (scale * scale);
        self.score = score as i32;
        self.credits = credits as i32;
    }

    /// A captured or authored balance for a test.
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_credits_for_test(&mut self, credits: i32) {
        self.credits = credits;
    }

    /// A captured or authored spending statistic for a test.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_spent_for_test(&mut self, spent: i32) {
        self.spent_credits = spent;
    }

    /// A captured or authored score for a test.
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_score_for_test(&mut self, score: i32) {
        self.score = score;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn rows(section: &str) -> Vec<Value> {
        let data: Value =
            serde_json::from_str(crate::test_fixture::text("tools/house_money_oracle.json"))
                .unwrap();
        data[section].as_array().unwrap().clone()
    }

    fn int(value: &Value) -> i32 {
        i32::try_from(value.as_i64().unwrap()).unwrap()
    }

    #[test]
    fn spend_money_matches_the_original() {
        for row in rows("spend_money") {
            let mut wallet = Economy::new(int(&row["balance"]));
            wallet.set_spent_for_test(int(&row["spent"]));
            wallet.spend_money(int(&row["amount"]));
            let expected = &row["expected"];
            assert_eq!(
                (wallet.credits(), wallet.spent_credits()),
                (int(&expected["balance"]), int(&expected["spent"])),
                "{}",
                row["name"]
            );
        }
    }

    #[test]
    fn add_credits_matches_the_original() {
        for row in rows("add_credits") {
            let mut wallet = Economy::new(int(&row["balance"]));
            wallet.add_credits(int(&row["amount"]));
            assert_eq!(
                wallet.credits(),
                int(&row["expected"]["balance"]),
                "{}",
                row["name"]
            );
        }
    }

    #[test]
    fn add_tiberium_credits_matches_the_original() {
        for row in rows("add_tiberium_credits") {
            let mut wallet = Economy::new(int(&row["balance"]));
            wallet.set_score_for_test(int(&row["score"]));
            let bales = int(&row["bales"]);
            wallet.add_tiberium_credits(
                int(&row["value"]) * bales,
                bales,
                row["scale_ppm"].as_i64().unwrap(),
                row["income_ppm"].as_i64().unwrap(),
            );
            let expected = &row["expected"];
            let mut balance = int(&expected["balance"]);
            if row["name"] == "IncomeMult 0.9 is a float" {
                // The documented float residual: ftol(25 * 0.9f * 40) = 899.
                assert_eq!(balance, 899);
                balance = 900;
            }
            assert_eq!(
                (wallet.credits(), wallet.score()),
                (balance, int(&expected["score"])),
                "{}",
                row["name"]
            );
        }
    }

    #[test]
    fn available_money_matches_the_original() {
        for row in rows("available_money") {
            let wallet = Economy::new(int(&row["balance"]));
            assert_eq!(
                wallet.available_money(),
                int(&row["expected"]),
                "{}",
                row["name"]
            );
        }
    }

    #[test]
    fn fresh_fill_silos_money_arm_matches_the_original() {
        let mut compared = 0;
        for row in rows("fill_silos") {
            // The flag belongs to the scenario caller; its false arm is
            // covered through the actual map loader in the retail chain.
            if !row["enabled"].as_bool().unwrap() {
                continue;
            }
            let mut wallet = Economy::new(int(&row["balance"]));
            wallet.set_score_for_test(int(&row["score"]));
            wallet.set_spent_for_test(int(&row["spent"]));
            wallet.fill_silos_fresh_empty_store(
                int(&row["value"]),
                int(&row["capacity"]),
                row["income_ppm"].as_i64().unwrap(),
            );
            let expected = &row["expected"];
            assert_eq!(
                (wallet.credits(), wallet.score(), wallet.spent_credits()),
                (
                    int(&expected["balance"]),
                    int(&expected["score"]),
                    int(&expected["spent"])
                ),
                "{}",
                row["name"]
            );
            compared += 1;
        }
        assert_eq!(compared, 8);
    }
}
