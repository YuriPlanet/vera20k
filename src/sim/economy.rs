//! Sole per-house cash balance and economy statistics, owned by `HouseState`.
//!
//! The retained purifier count is an end-of-frame hash projection; deposits
//! count current buildings and the AI virtual bonus through their own producer.
//! `IncomeMult` is read per-deposit from the house's country type. Never depends on
//! render/ui/sidebar/audio/net (sim invariant #1).
//! The balance and statistics are serialized and hashed. Factory kernels borrow
//! this value directly; income, repair and other house consumers use the same cash.

/// Per-house wallet and statistics. There is no second house credit balance.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Economy {
    /// Sole spendable cash balance.
    pub credits: i32,
    /// Running total charged through the factory spending path.
    pub spent_credits: i32,
    /// Ore-deposit x5.0 statistics accumulator.
    pub harvested_credits: i32,
    /// Retained end-of-frame OrePurifier building-count projection used by the
    /// existing hash schema. Gameplay deposits count live buildings instead.
    pub purifier_count: i32,
}

impl Economy {
    /// Add a factory refund using its existing saturating arithmetic.
    /// Native direct income currently uses wrapping arithmetic in `credit_income`.
    /// Unifying these operations requires a separate behavior correction.
    pub fn add_credits(&mut self, amount: i32) {
        self.credits = self.credits.saturating_add(amount);
    }

    /// Accumulate the statistics x5.0 figure for `bales` deposited. Integer `*5`
    /// because bales are integral (the engine's deposit x5.0 truncates to integer).
    /// Statistics only — does NOT touch `credits`.
    pub fn add_harvested(&mut self, bales: i32) {
        self.harvested_credits = self
            .harvested_credits
            .saturating_add(bales.saturating_mul(5));
    }

    /// Accumulate a PRE-COMPUTED HarvestedCredits figure (already the x5.0 product). The
    /// OrePurifier-bonus stat term is `trunc(count * 0.25 * amount * 5.0)` computed in ONE
    /// step (NOT floor-the-bonus-bales-then-x5), so the caller passes the finished value.
    /// Statistics only — never touches `credits`.
    pub fn add_harvested_raw(&mut self, harvested: i32) {
        self.harvested_credits = self.harvested_credits.saturating_add(harvested);
    }

    /// Factory charge: spend up to `amount`, returning the amount paid and
    /// accumulating its spending statistic. This preserves the existing bounded
    /// factory operation; the native income module's spending path is distinct.
    pub fn spend(&mut self, amount: i32) -> i32 {
        let paid = amount.max(0).min(self.credits.max(0));
        self.credits -= paid;
        self.spent_credits = self.spent_credits.saturating_add(paid);
        paid
    }

    /// Spendable balance.
    pub fn available(&self) -> i32 {
        self.credits
    }
}

/// Parts-per-million scale for `IncomeMult` (1_000_000 = 1.0×). MUST equal
/// `rules::ruleset::INCOME_PPM_SCALE` (kept local so this module stays `std`-only).
pub const INCOME_PPM_SCALE: i64 = 1_000_000;

/// Apply an `IncomeMult` (parts-per-million; `INCOME_PPM_SCALE` = 1.0×) to a non-negative
/// credit amount, truncating toward zero — matching gamemd's single `ftol` per deposit
/// call. The `i64` intermediate avoids overflow; the result saturates into `i32`. With
/// `income_ppm == INCOME_PPM_SCALE` this is the identity (so stock YR, where every country
/// is 1.0, is hash-neutral).
pub fn apply_income_mult(amount: i32, income_ppm: i64) -> i32 {
    let scaled = (amount as i64).saturating_mul(income_ppm) / INCOME_PPM_SCALE;
    scaled.clamp(i32::MIN as i64, i32::MAX as i64) as i32
}

/// Per-slot OrePurifier BONUS credits — gamemd's second `Add_Tiberium_Credits` call:
/// `trunc(base_value × IncomeMult × purifier_count × PurifierBonus)` as ONE truncation.
/// Both `IncomeMult` (`income_ppm`) and `PurifierBonus` (`bonus_ppm`) are ppm fractions
/// (`INCOME_PPM_SCALE` = 1.0×) folded inside a single floor — matching gamemd's one `ftol`;
/// truncating either factor separately first would drift ±1. An i128 intermediate is required
/// because the two ppm factors overflow i64. `base_value` is the slot's total ore/gem credit
/// value (Σ bale values). Returns 0 for ≤0 purifiers.
pub fn purifier_bonus_credits(
    base_value: i32,
    purifier_count: i32,
    bonus_ppm: i64,
    income_ppm: i64,
) -> i32 {
    if purifier_count <= 0 {
        return 0;
    }
    const SCALE_SQUARED: i128 = (INCOME_PPM_SCALE as i128) * (INCOME_PPM_SCALE as i128);
    let v = (base_value as i128)
        * (purifier_count as i128)
        * (bonus_ppm as i128)
        * (income_ppm as i128)
        / SCALE_SQUARED;
    v.clamp(i32::MIN as i128, i32::MAX as i128) as i32
}

/// Per-slot OrePurifier BONUS HarvestedCredits stat: `trunc(purifier_count × PurifierBonus ×
/// bales × 5.0)` as ONE truncation (`bonus_ppm` is the ppm fraction, `INCOME_PPM_SCALE` =
/// 1.0×; the `×5` baked in — NOT floor-bonus-bales-then-×5). Statistics only; 0 for ≤0
/// purifiers.
pub fn purifier_bonus_harvested(bales: i32, purifier_count: i32, bonus_ppm: i64) -> i32 {
    if purifier_count <= 0 {
        return 0;
    }
    let v = (bales as i64) * (purifier_count as i64) * bonus_ppm * 5 / INCOME_PPM_SCALE;
    v.clamp(i32::MIN as i64, i32::MAX as i64) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn economy_default_is_zeroed() {
        let e = Economy::default();
        assert_eq!(
            (
                e.credits,
                e.spent_credits,
                e.harvested_credits,
                e.purifier_count
            ),
            (0, 0, 0, 0)
        );
    }

    #[test]
    fn economy_add_credits_accumulates() {
        let mut e = Economy::default();
        e.add_credits(500);
        e.add_credits(250);
        assert_eq!(e.credits, 750);
        assert_eq!(e.available(), 750);
    }

    /// The x5.0 statistics
    /// accumulator truncates to integer `*5` and never touches credits.
    #[test]
    fn economy_add_harvest_truncates_x5() {
        let mut e = Economy::default();
        e.add_harvested(7);
        assert_eq!(e.harvested_credits, 35);
        assert_eq!(e.credits, 0, "harvested stat must not move credits");
    }

    /// Isolated method test: spend deducts up to the balance, returns the paid
    /// amount, never goes negative, and tracks spent_credits. The silo-drain
    /// fallback is not implemented by this bounded factory operation.
    #[test]
    fn economy_spend_caps_at_balance_and_tracks_spent() {
        let mut e = Economy::default();
        e.add_credits(100);
        assert_eq!(e.spend(30), 30);
        assert_eq!(e.credits, 70);
        assert_eq!(e.spent_credits, 30);
        // Over-spend is capped at the balance; never negative.
        assert_eq!(e.spend(1000), 70);
        assert_eq!(e.credits, 0);
        assert_eq!(e.spent_credits, 100);
    }

    // ===== P7 — income wiring =====

    /// IncomeMult identity at 1.0× (stock): apply_income_mult is the identity, so stock YR
    /// (every country 1.0) is hash-neutral.
    #[test]
    fn apply_income_mult_identity_at_one() {
        assert_eq!(apply_income_mult(123, INCOME_PPM_SCALE), 123);
        assert_eq!(apply_income_mult(0, INCOME_PPM_SCALE), 0);
    }

    /// IncomeMult truncates toward zero (one ftol), matching gamemd.
    #[test]
    fn apply_income_mult_truncates_toward_zero() {
        assert_eq!(apply_income_mult(25, 1_200_000), 30); // 30.0
        assert_eq!(apply_income_mult(7, 1_200_000), 8); // trunc(8.4)
        assert_eq!(apply_income_mult(1000, 1_200_000), 1200);
    }

    /// The BLOCKER parity case: the purifier bonus is ONE truncation folding IncomeMult
    /// inside. gem slot_value 50, 3 purifiers @25%, IncomeMult 1.2 -> 45 (gamemd), NOT 44
    /// (the double-truncation `trunc(1.2 × trunc(50×3×25/100=37)) = 44` the design's first
    /// draft produced).
    #[test]
    fn purifier_bonus_credits_single_truncation_not_double() {
        assert_eq!(purifier_bonus_credits(50, 3, 250_000, 1_200_000), 45);
        // Sanity: the buggy double-trunc would have been 44.
        let double_trunc = apply_income_mult((50 * 3 * 25) / 100, 1_200_000);
        assert_eq!(double_trunc, 44, "documents the bug this guards against");
        assert_ne!(
            purifier_bonus_credits(50, 3, 250_000, 1_200_000),
            double_trunc
        );
    }

    /// At IncomeMult 1.0 the bonus equals the legacy `slot_value×count×pct/100` (so stock
    /// is hash-neutral), and 0 purifiers -> 0.
    #[test]
    fn purifier_bonus_credits_stock_and_zero() {
        assert_eq!(
            purifier_bonus_credits(100, 2, 250_000, INCOME_PPM_SCALE),
            50
        );
        assert_eq!(purifier_bonus_credits(100, 0, 250_000, INCOME_PPM_SCALE), 0);
        assert_eq!(purifier_bonus_credits(100, -1, 250_000, 1_200_000), 0);
    }

    /// The bonus HarvestedCredits stat is trunc(count × 0.25 × bales × 5) in one step:
    /// count 1, 1 bale -> trunc(1.25) = 1 (NOT floor(0.25)×5 = 0).
    #[test]
    fn purifier_bonus_harvested_single_truncation() {
        assert_eq!(purifier_bonus_harvested(1, 1, 250_000), 1);
        assert_eq!(purifier_bonus_harvested(4, 1, 250_000), 5); // trunc(0.25×4×5=5.0)
        assert_eq!(purifier_bonus_harvested(10, 2, 250_000), 25); // 10×2×0.25×5
        assert_eq!(purifier_bonus_harvested(10, 0, 250_000), 0);
    }

    /// add_harvested_raw adds a pre-computed figure without the ×5 (used for the bonus
    /// stat, which is already the ×5 product); never touches credits.
    #[test]
    fn add_harvested_raw_adds_without_x5() {
        let mut e = Economy::default();
        e.add_harvested_raw(7);
        assert_eq!(e.harvested_credits, 7);
        assert_eq!(e.credits, 0);
    }

    /// Full-precision ppm flows through the credit fold: PurifierBonus=.333 (333_000 ppm),
    /// 3 purifiers, base 1000, IncomeMult 1.0 -> trunc(1000 × 3 × 0.333) = 999. The old
    /// whole-percent path (33% = 330_000) would give trunc(1000×3×0.33) = 990 — a
    /// player-visible 9-credit gap per deposit. Stock .25 (250_000) stays byte-identical.
    #[test]
    fn purifier_bonus_credits_fractional_ppm_beats_whole_percent() {
        assert_eq!(
            purifier_bonus_credits(1000, 3, 333_000, INCOME_PPM_SCALE),
            999
        );
        assert_eq!(
            purifier_bonus_credits(1000, 3, 330_000, INCOME_PPM_SCALE),
            990
        );
        assert_eq!(
            purifier_bonus_credits(1000, 2, 250_000, INCOME_PPM_SCALE),
            500
        );
    }
}
