//! What the house may spend, and what it refuses to.
//!
//! Every number here is a *permission*. None of them buys anything: the build
//! list in [`super::build`] and the production lines in [`super::composition`]
//! still ask the wallet, and a permission the wallet cannot cover buys nothing.
//! That split is the point. Before this module the thresholds lived in three
//! files — the credit floors beside `build.rs`'s yard logic, the "rich" test
//! inside `composition.rs`'s line choice, the gates in `plan.rs` — and a change
//! to what the house considers affordable had to be made in all three or the
//! three disagreed. The question "can this wait?" has one owner now.
//!
//! Two kinds of answer live here. There are **floors** — [`TECH_START_CREDITS`],
//! [`SUPPORT_MIN_CREDITS`], [`SURPLUS_CREDITS`] — which are credit amounts and
//! read as amounts because that is what a price tag is. And there is
//! **patience** — [`reserve`], [`SURPLUS_INCOME_PER_MINUTE`] — which is measured
//! against what the house earns rather than what it happens to hold, because the
//! same number is a different amount of patience to a house earning 1 200 a
//! minute and one earning 12 000. A house that opens with ten thousand credits
//! instead of a hundred thousand should feel it for the opening and not for the
//! whole match, and a threshold written as a credit amount feels it for the
//! whole match.

use crate::rules::object_type::ObjectCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::control::observation::Observation;

use super::build::class_types;

/// The free wallet the tech stage needs before it answers: the price of the
/// cheapest thing it buys.
///
/// `[AMRADR] Cost=1000` is that price, and the stage's dearest entry is the
/// lab at `[GATECH] Cost=2000`. The gate used to ask for the dearest one in
/// hand, and a house never held it: the barracks and the war factory spend
/// every credit above their own floors, so the wallet the lab needed was one
/// frame away for the whole match — the report is an army of infantry and no
/// lab, no radar, no tank. At the cheap step's price the stage is decided
/// instead, and `build_next` holds the difference back from the lines
/// (`yard_reserve`) so the wallet can fill. The hold expires with
/// [`BUILD_HOLD_FRAMES`], so a house that cannot fill it is not saving
/// forever.
pub(super) const TECH_START_CREDITS: i32 = 1_000;

/// The free wallet a building on the yard maker's `Prerequisite=` list has to
/// hold: it buys the ability to rebuild a lost yard, which only pays off once
/// the base can afford to lose one.
pub(super) const SUPPORT_MIN_CREDITS: i32 = 4_000;

/// The free wallet a house has to be holding before it treats a factory as the
/// bottleneck: past this it cannot spend what it earns, so the answer is
/// another factory rather than another harvester.
pub(super) const SURPLUS_CREDITS: i32 = 12_000;

/// The income one production line can spend, in credits a minute, and so the
/// income at which a second line is worth buying.
///
/// A line turns out a unit in its `Cost_Of` worth of frames: a 900-credit tank
/// takes about forty-five seconds to build, which is 1 200 credits a minute.
/// Two lines spend about 2 400. Past that the house is earning faster than its
/// factories can turn it into army, and the wallet it cannot spend is the
/// signal — not a round number the wallet happens to reach once.
pub(super) const SURPLUS_INCOME_PER_MINUTE: i32 = 2_400;

/// The price of the cheapest line vehicle the house can build now.
///
/// Three questions read it, and they are the same question from three sides:
/// what the barracks is saving the wallet for, what the war factory has to be
/// able to pay before it has anything to make, and what the house must always
/// be able to replace ([`reserve`]). It is read from the rules rather than
/// fixed at a constant: a fixed floor is the price of a *heavy* unit, and a war
/// factory held to it makes nothing while the barracks spends every credit
/// above its own floor — the army that is all infantry and never a tank. A side
/// whose line tank is dearer than another's answers with its own price this
/// way.
pub(super) fn line_vehicle_floor(rules: &RuleSet, obs: &Observation<'_>) -> Option<i32> {
    let mut core = None;
    let mut fallback = None;
    for (ty,_) in class_types(rules,obs,ObjectCategory::Vehicle) {
        if ty.naval || !super::orders::is_combat_unit(rules,ty) { continue; }
        let price = obs.cost_of(rules,ty);
        if price <= 0 { continue; }
        fallback = Some(fallback.map_or(price, |v:i32| v.min(price)));
        if super::mobilization::line_quality(rules,ty) > 0 {
            core = Some(core.map_or(price, |v:i32| v.min(price)));
        }
    }
    core.or(fallback)
}

/// How much of the wallet the house refuses to spend: one minute of its own
/// income, and never less than one line vehicle.
///
/// Every threshold in this module and in [`super::plan`] is a permission, not a
/// purchase — `build_next` still refuses what the wallet cannot cover. What was
/// missing was the floor under the wallet. A house that spends down to zero
/// every frame is one the next bad buy cannot be recovered from, and a house
/// that opens with ten thousand credits instead of a hundred thousand feels it
/// for the whole match: the plants and the bays take the money, and the army is
/// never paid for.
///
/// Read against income rather than as a credit number, for the reason
/// [`SURPLUS_INCOME_PER_MINUTE`] is: the same number is a different amount of
/// patience to a house earning 1 200 a minute and one earning 12 000. A minute
/// of income is the unit the question "can this wait?" is actually asked in.
///
/// The floor is one line vehicle because it is the cheapest thing the house must
/// always be able to replace; under it the reserve would permit a house to be
/// talked out of the tank it is saving for.
pub(super) const RESERVE_SECONDS: i32 = 60;

/// The least the house holds back, in credits: one line vehicle's price.
///
/// The fallback for a side whose rules give no combat vehicle, and the answer
/// before the income meter has a window.
pub(super) const MIN_RESERVE_CREDITS: i32 = 700;

/// What the house refuses to spend this frame.
///
/// A minute of measured income, floored at one line vehicle — and nothing at
/// all before the meter has a window. A house whose income has not been
/// measured is not held to a reserve it cannot size; twelve seconds in, the
/// meter has one and the floor applies.
pub(super) fn reserve(rules: &RuleSet, obs: &Observation<'_>, income: Option<i32>) -> i32 {
    if !income.is_some_and(|rate|rate>0) {return 0;}
    let floor = line_vehicle_floor(rules, obs).unwrap_or(MIN_RESERVE_CREDITS);
    cash_reserve(floor,income)
}

fn cash_reserve(floor:i32,income:Option<i32>)->i32 {
    // A measured zero is not merely an unknown opening income. Do not
    // permanently immobilise a finite wallet in a stranded economy.
    let Some(rate)=income.filter(|rate|*rate>0) else {return 0;};
    rate.saturating_mul(RESERVE_SECONDS)
        .saturating_div(60)
        .max(floor)
}

/// How long the build list's answer stands before it may answer a different
/// entry, in frames: forty seconds (600 frames at the 15 a second the
/// simulation steps).
///
/// Long enough to save for a structure the wallet cannot cover the frame it is
/// chosen — a gate opens on a wallet that is then spent elsewhere — and short
/// enough that a base which really has changed its mind (the defense it wanted
/// now stands) is not held to a plan for a minute.
pub(super) const BUILD_HOLD_FRAMES: u64 = 600;

/// Hold the entire price of a selected, not-yet-funded investment. A deficit
/// is NOT a reserve: with 1900/2000 saved, holding just 100 spends the 1900.
pub(super) fn investment_hold(free: i32, price: i32) -> i32 {
    if free < price { price.max(0) } else { 0 }
}

/// Is a non-emergency bank plausibly fundable in the next three minutes?
/// Future income is a scheduling hint, NEVER treated as spendable credits.
/// Unknown income permits normal opening/essential recovery but not optional
/// indefinite saving. A funded purchase needs no forecast.
pub(super) fn can_save_for(free: i32, price: i32, income: Option<i32>) -> bool {
    free >= price || income.is_some_and(|rate| rate > 0 &&
        i64::from(price).saturating_sub(i64::from(free)) <= i64::from(rate)*3)
}

/// A selected investment can protect real cash for at most one minute.
/// A failed or displaced bank gives ordinary production another 30 seconds;
/// choosing the same item again cannot renew its original deadline.
#[derive(Debug, Default)]
pub(super) struct InvestmentBank {
    target: Option<(crate::sim::intern::InternedId, u64)>,
    retry_after: u64,
}

impl InvestmentBank {
    pub(super) fn active(&self, now: u64) -> Option<crate::sim::intern::InternedId> {
        self.target.filter(|(_,until)| now < *until).map(|(id,_)| id)
    }

    pub(super) fn clear(&mut self) { self.target = None; }

    /// Losing an unfunded optional commitment is not completing it. Keep the
    /// same cooldown as an expired bank, even if another pass displaced it.
    pub(super) fn interrupt(&mut self, now: u64) {
        if self.target.take().is_some() {
            self.retry_after = self.retry_after.max(now.saturating_add(450));
        }
    }

    pub(super) fn hold(&mut self, now: u64, id: crate::sim::intern::InternedId,
        free: i32, price: i32, income: Option<i32>) -> i32 {
        self.hold_for(now,id,free,price,income,900,1,false)
    }

    /// An income-producing purchase can wait for up to two minutes of
    /// measured income. It still expires and cannot borrow future credits.
    pub(super) fn hold_income(&mut self,now:u64,id:crate::sim::intern::InternedId,
        free:i32,price:i32,income:Option<i32>,waiting:bool)->i32 {
        self.hold_for(now,id,free,price,income,1800,2,waiting)
    }

    fn hold_for(&mut self,now:u64,id:crate::sim::intern::InternedId,
        free:i32,price:i32,income:Option<i32>,frames:u64,minutes:i64,waiting:bool)->i32 {
        if free >= price && !waiting { self.clear(); return 0; }
        if let Some((selected, until)) = self.target {
            if selected != id || now >= until {
                self.interrupt(now);
                return 0;
            }
        }
        if free<price && !income.is_some_and(|rate| rate > 0
            && i64::from(price) - i64::from(free) <= i64::from(rate)*minutes)
        {
            self.interrupt(now);
            return 0;
        }
        if now < self.retry_after { return 0; }
        if self.target.is_none() {
            self.target = Some((id,now.saturating_add(frames)));
        }
        price.max(0)
    }
}

/// Discretionary purchases must leave the next useful line vehicle funded
/// on a functioning but thin economy. Urgent recovery/defence is handled by
/// its own earlier admission. Rich economies can spend their true surplus.
pub(super) fn discretionary_floor(free: i32, income: Option<i32>, line: i32) -> i32 {
    // An impossible second bank must not freeze affordable infantry/utility
    // production after the primary reserve has already been deducted.
    if free<line && income.is_some()
        && !can_save_for(free,line,income) {return 0;}
    if free >= line.saturating_mul(6) && income.is_some_and(|v| v >= 1200) { 0 }
    else { line.max(0) }
}

#[cfg(all(test, tests_sim_p_s))]
mod v50_tests {
    use super::*;
    #[test]
    fn income_bank_waits_for_a_real_busy_line_but_expires() {
        let id=crate::sim::intern::InternedId::from_index(1);
        let mut bank=InvestmentBank::default();
        assert_eq!(bank.hold_income(0,id,400,1400,Some(600),false),1400);
        assert_eq!(bank.hold_income(1200,id,1400,1400,None,true),1400);
        assert_eq!(bank.hold_income(1800,id,1400,1400,None,true),0);
        assert_eq!(bank.hold_income(1801,id,1400,1400,None,false),0);
        assert_eq!(bank.active(1801),None);
    }
    #[test]
    fn bounded_quality_bank_expires_and_cannot_borrow() {
        let id=crate::sim::intern::InternedId::from_index(1);
        let mut bank=InvestmentBank::default();
        assert_eq!(bank.hold(0,id,-100,1200,Some(1250)),0);
        assert_eq!(bank.hold(0,id,500,1200,None),0);
        assert_eq!(bank.hold(0,id,500,1200,Some(800)),1200);
        assert_eq!(bank.hold(899,id,500,1200,Some(800)),1200);
        assert_eq!(bank.hold(900,id,500,1200,Some(800)),0);
        assert_eq!(bank.hold(1349,id,500,1200,Some(800)),0);
        assert_eq!(bank.hold(1350,id,500,1200,Some(800)),1200);
        assert_eq!(bank.hold(1351,id,1200,1200,Some(800)),0);
        assert_eq!(bank.active(1351),None);
    }
    #[test]
    fn displaced_optional_bank_cannot_restart_its_deadline_next_frame() {
        let id=crate::sim::intern::InternedId::from_index(1);
        let mut bank=InvestmentBank::default();
        assert_eq!(bank.hold(0,id,500,1200,Some(800)),1200);
        bank.interrupt(30);
        assert_eq!(bank.active(30),None);
        assert_eq!(bank.hold(31,id,500,1200,Some(800)),0);
        bank.interrupt(100); // no active bank: do not extend cooldown forever
        assert_eq!(bank.hold(479,id,500,1200,Some(800)),0);
        assert_eq!(bank.hold(480,id,500,1200,Some(800)),1200);
    }
    #[test]
    fn an_investment_protects_saved_cash_not_only_its_deficit() {
        assert_eq!(investment_hold(1900,2000),2000);
        assert_eq!(investment_hold(0,2000),2000);
        assert_eq!(investment_hold(2000,2000),0);
    }
    #[test]
    fn an_impossible_bank_does_not_freeze_a_stranded_house() {
        assert!(!can_save_for(500,2000,Some(0)));
        assert!(!can_save_for(500,2000,Some(100)));
        assert!(can_save_for(500,2000,Some(500)));
        assert!(can_save_for(2000,2000,None));
    }
    #[test]
    fn discretionary_spending_preserves_a_core_cycle_until_prosperous() {
        assert_eq!(discretionary_floor(1900,Some(300),900),900);
        assert_eq!(discretionary_floor(5400,Some(1200),900),0);
        assert_eq!(discretionary_floor(400,Some(0),900),0);
        assert_eq!(discretionary_floor(400,Some(100),900),0);
        assert_eq!(discretionary_floor(400,Some(300),900),900);
        assert_eq!(discretionary_floor(400,None,900),900);
    }
    #[test]
    fn a_confirmed_stranded_wallet_is_not_frozen_by_a_permanent_cash_reserve() {
        assert_eq!(cash_reserve(700,Some(0)),0);
        assert_eq!(cash_reserve(700,None),0);
        assert_eq!(cash_reserve(700,Some(300)),700);
        assert_eq!(cash_reserve(700,Some(1800)),1800);
    }
}
