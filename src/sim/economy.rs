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

use crate::util::native_x87::{MaskedX87Chop53 as X87, NativeF32Bits};

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
        income: NativeF32Bits,
    ) {
        let mut budget = self.available_money();
        // Preserve signed comparison and SUB32, including custom zero/negative
        // Values. A division shortcut or safety clamp changes native behavior.
        while budget > value && capacity > 0 {
            self.add_tiberium_credits(value, NativeF32Bits::ONE, income);
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
    /// of a tiberium. Each operation follows4F9610..4F9664 in native PC53/chop
    /// order, with the caller's stored float amount and HouseType+148 income.
    /// The signed per-type Value is read at payment, not summed from cargo.
    /// Both sums include their prior integer before _ftol's low32 result.
    pub(crate) fn add_tiberium_credits(
        &mut self,
        value: i32,
        amount: NativeF32Bits,
        income: NativeF32Bits,
    ) {
        let amount = X87::load_f32(amount);
        self.score = X87::ftol_i32_low_masked(X87::add(
            X87::mul(amount, X87::load_i32(5)),
            X87::load_i32(self.score),
        ));
        self.credits = X87::ftol_i32_low_masked(X87::add(
            X87::mul(
                X87::mul(X87::load_i32(value), X87::load_f32(income)),
                amount,
            ),
            X87::load_i32(self.credits),
        ));
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

    /// Historical oracle supplies already-stored floats from Python pack('f').
    /// INI reader narrowing is exercised separately through RuleSet.
    fn supplied_float(value: f64) -> NativeF32Bits {
        NativeF32Bits::from_bits((value as f32).to_bits())
    }

    #[test]
    fn original_raw_tiberium_credit_calls_match_wallet_and_score() {
        let corpus = crate::test_fixture::json("tools/spatial_oracle/refinery_income.json");
        let bits = |value: &Value| {
            NativeF32Bits::from_bits(u32::from_str_radix(value.as_str().unwrap(), 16).unwrap())
        };
        let rows = corpus["primitive_rows"].as_array().unwrap();
        assert_eq!(rows.len(), 191);
        for row in rows {
            let input = &row["input"];
            let mut wallet = Economy::new(int(&input["balance"]));
            wallet.set_score_for_test(int(&input["score"]));
            wallet.set_spent_for_test(7);
            wallet.add_tiberium_credits(
                int(&input["values"][input["slot"].as_u64().unwrap() as usize]),
                bits(&input["amount_bits"]),
                bits(&input["income_bits"]),
            );
            assert_eq!(
                (wallet.credits(), wallet.score(), wallet.spent_credits()),
                (
                    int(&row["output"]["balance"]),
                    int(&row["output"]["score"]),
                    int(&row["output"]["spent"]),
                ),
                "{}",
                input["name"]
            );
        }
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
                int(&row["value"]),
                supplied_float(f64::from(bales) * row["scale_ppm"].as_f64().unwrap() / 1e6),
                supplied_float(row["income_ppm"].as_f64().unwrap() / 1e6),
            );
            let expected = &row["expected"];
            assert_eq!(
                (wallet.credits(), wallet.score()),
                (int(&expected["balance"]), int(&expected["score"])),
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
                supplied_float(row["income_ppm"].as_f64().unwrap() / 1e6),
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
