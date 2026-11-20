//! The house's wallet, owned by `HouseState`: its cash balance (House
//! `+0x30C`), the spending statistic (`+0x2DC`) and the score (`+0x54E8`).
//! Every writer goes through the money primitives here, each the one port of
//! its native function.
//!
//! The house's silo aggregate (`+0x2FC`) is not represented here. Spend_Money
//! omits its silo-drain arm; Available_Money omits its storage-value term
//! and assumes a finite native HouseType `IncomeMult`. The corresponding
//! tools/house_money_oracle.py controls supply an empty aggregate.
//! `Added_To_Game @ 0x00502B83` merges a building's store (`+0x33C`) into
//! that same aggregate, which VERA still omits. Raw native Save/Load preserves
//! these slots, so the empty-store boundary must not be extended to arbitrary
//! native saves.
//!
//! The separate house weed store `+0x314` *is* represented here: four float
//! slots and `HouseClass::GiveWeed @ 0x004F9700`, its only writer, reached from
//! `UnitClass::Mission_Unload @ 0x0073E474..0x0073E493` when the unloader's
//! type has `Weeder=` (`Type+0xE0F`). Native credits that deposit to the
//! *unloader's own* House (`Unit+0x21C`), unlike the ordinary tiberium arm
//! which pays the refinery building's owner. `Rules+0x17D0`
//! (`[General] WeedCapacity`, `ReadGeneral @ 0x0067194D..0x00671967`)
//! bounds the whole store, not one slot.
//!
//! RESIDUAL: the weed source terrain, the `Weeder=` harvest branches
//! (`0x0073D4F4`, `0x0073E76C`), `UseWeeds=` superweapon drain,
//! `HouseClass @ 0x004F9750` (the total/capacity ratio) and the Phobos
//! `WeedsCounter` sidebar indicator are separate chains. With retail rules no
//! type sets `Weeder=`, so this store stays at zero unless a custom rules set
//! supplies one.
//!
//! Ares fills those stores (`Refinery.UseStorage`, Ares
//! `HouseClass_GiveTiberium_Storage @ 10024290`), so the wallet keeps them
//! (`sim::ares_tiberium_storage::AresTiberiumStorage`), empty unless the
//! game's money takes its Ares form (`RuleSet::ares_form`). The two arms
//! that read them are ported for it: Spend_Money's silo drain and
//! Available_Money's storage value. They run in every game and keep the
//! empty boundary while the stores are empty.
//!
//! `[Basic] FillSilos=` (`Fill_In_Data 0x00684F02..0x00684F7C`) pays through
//! GiveTiberium at a scenario's start (`sim::scenario_post_map`).
//!
//! Never depends on render/ui/audio/net (sim invariant #1). The wallet is
//! serialized and hashed.

use crate::sim::ares_tiberium_storage::AresTiberiumStorage;
use crate::sim::storage_class::StorageAmounts;
use crate::util::native_x87::{MaskedX87Chop53 as X87, MaskedX87Ordering, NativeF32Bits};

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
    /// The House store (`+0x2FC`) and its buildings' stores, which only
    /// Ares fills; empty unless the money takes its Ares form.
    #[serde(default)]
    storage: AresTiberiumStorage,
    /// House `+0x314`, the TS-era weed store (`StorageClass`, four
    /// per-Tiberium floats). `HouseClass::Constructor @ 0x004F57F6` clears
    /// it; `GiveWeed @ 0x004F9700` is its only writer in YR.
    #[serde(default)]
    owned_weed: StorageAmounts,
}

impl Economy {
    /// A wallet holding `credits`: `HouseClass::Set_Credits_And_Color @
    /// 0x004FCE00` seeds the balance of a new house. RESIDUAL (invisible): it
    /// also writes `+0x1DC` (`scenario_credits`), which stays 0 here for
    /// multiplayer houses; no live multiplayer path reads it.
    pub(crate) const fn new(credits: i32) -> Self {
        Self {
            credits,
            scenario_credits: 0,
            spent_credits: 0,
            score: 0,
            storage: AresTiberiumStorage::new(),
            owned_weed: StorageAmounts::EMPTY,
        }
    }

    /// Ares' tiberium storage (`sim::ares_tiberium_storage`), empty unless
    /// the money takes its Ares form.
    pub(crate) fn ares_storage(&self) -> &AresTiberiumStorage {
        &self.storage
    }

    pub(crate) fn ares_storage_mut(&mut self) -> &mut AresTiberiumStorage {
        &mut self.storage
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
    /// Native saves and a nonfinite income remain outside the represented
    /// domain; the two consumed oracle controls use empty storage.
    ///
    /// When the money takes its Ares form the stored tiberium's value is
    /// added ([`AresTiberiumStorage::available_money`]). One body serves
    /// every game: only Ares fills the store, and an empty store leaves the
    /// balance, as above.
    pub fn available_money(&self) -> i32 {
        self.storage.available_money(self.credits)
    }

    /// `HouseClass::Add_Credits @ 0x004F9950..0x004F9962`: the complete body
    /// adds the signed argument to House+30C modulo2^32 and writes only that
    /// balance. No score/spending, timer, RNG, detach or callback side effect.
    pub fn add_credits(&mut self, amount: i32) {
        self.credits = self.credits.wrapping_add(amount);
    }

    /// EvadeClass::Do @0x004C6272..0x004C6285 calls Add_Credits and then
    /// adds the same signed amount to House+1DC. Score/spending stay unchanged.
    pub(crate) fn add_campaign_carry_over(&mut self, amount: i32) {
        self.add_credits(amount);
        self.scenario_credits = self.scenario_credits.wrapping_add(amount);
    }

    /// `HouseClass::Spend_Money @ 0x004F9790`. An amount the balance covers
    /// (signed `<=`) comes off it; otherwise the whole balance, even a
    /// negative one, is taken and zeroed, and the rest would drain the silos
    /// (`0x004F97DD..`, empty here). What was taken joins `+0x2DC`.
    ///
    /// When the money takes its Ares form that drain finds units
    /// (`AresTiberiumStorage::drain_for_spend`): the stored units it sells
    /// join what was taken, and the last unit's excess is the new balance.
    pub(crate) fn spend_money(&mut self, amount: i32) {
        let taken = if amount <= self.credits {
            self.credits = self.credits.wrapping_sub(amount);
            amount
        } else {
            let taken = std::mem::take(&mut self.credits);
            // Runs in every game: an empty store, which only Ares fills,
            // drains nothing and leaves the balance at 0.
            let (drained, change) = self.storage.drain_for_spend(amount.wrapping_sub(taken));
            self.credits = change;
            taken.wrapping_add(drained)
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
    /// of a tiberium: the stock GiveTiberium body, which its callers reach
    /// through `Simulation::tiberium_gift`. Each operation follows
    /// 4F9610..4F9664 in native PC53/chop order, with the caller's stored
    /// float amount and HouseType+148 income.
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

    /// `HouseClass::GiveWeed @ 0x004F9700`, the sole writer of House+0x314.
    ///
    /// `count` is the whole-bale amount the unload gate removed from one
    /// cargo slot and `slot` is that Tiberium slot's index. The native loop
    /// (`0x004F9715..0x004F9748`) re-reads `StorageClass::GetTotalAmount
    /// @ 0x006C9650` **before every single** `Add_Amount(1.0, slot) @
    /// 0x006C9690`, and stops as soon as the total is no longer strictly less
    /// than `f32(Rules+0x17D0)` (`fild`/`fstp` at `0x004F971C..0x004F9722`,
    /// `fcomp` at `0x004F972B`). So a deposit larger than the remaining
    /// capacity is truncated mid-count, and a non-positive count or capacity
    /// adds nothing.
    pub(crate) fn give_weed(&mut self, count: i32, slot: usize, weed_capacity: i32) {
        if count <= 0 {
            return;
        }
        let mut remaining = count;
        loop {
            // 0x004F9715 re-reads Rules+0x17D0 and narrows it to f32 on
            // every iteration, before the GetTotalAmount call.
            let capacity = X87::store_f32_masked_chop(X87::load_i32(weed_capacity));
            let total = self.owned_weed.total();
            if X87::compare(X87::load_i32(total), X87::load_f32(capacity))
                != MaskedX87Ordering::Less
            {
                return;
            }
            self.owned_weed.add_amount(slot, NativeF32Bits::ONE);
            remaining -= 1;
            if remaining <= 0 {
                return;
            }
        }
    }

    /// `StorageClass::GetTotalAmount @ 0x006C9650` over House+0x314 — the
    /// value Phobos' `WeedsCounter` hook reads.
    pub fn owned_weed_total(&self) -> i32 {
        self.owned_weed.total()
    }

    /// `StorageClass::RemoveAmount @ 0x006C96B0` on House+0x314, reached only
    /// by the Phobos `UseWeeds` drain: the amount narrows to f32 and comes out
    /// of **slot 0 only**. `GetTotalAmount` compared the whole store before the
    /// call, so the other slots keep what the drain does not reach.
    pub(crate) fn remove_weed_amount(&mut self, amount: i32) -> NativeF32Bits {
        let amount = X87::store_f32_masked_chop(X87::load_i32(amount));
        self.owned_weed.remove_amount(amount, 0)
    }

    /// The four retained House+0x314 float slots, for the state hash and
    /// save/load diagnostics.
    pub(crate) const fn owned_weed_amounts(&self) -> [NativeF32Bits; 4] {
        self.owned_weed.amounts()
    }

    /// Drive the production `GiveWeed @ 0x004F9700` gate from a client test,
    /// capacity check included, rather than poking the retained slots.
    #[cfg(any(test, feature = "test-support"))]
    pub fn give_weed_for_test(&mut self, count: i32, slot: usize, weed_capacity: i32) {
        self.give_weed(count, slot, weed_capacity);
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

#[cfg(all(test, tests_sim_c_g))]
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
}
