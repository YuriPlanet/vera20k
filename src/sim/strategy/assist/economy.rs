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
    class_types(rules, obs, ObjectCategory::Vehicle)
        .filter(|(ty, _)| super::orders::is_combat_unit(rules, ty))
        .map(|(ty, _)| obs.cost_of(rules, ty))
        .min()
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
    let Some(rate) = income else {
        return 0;
    };
    let floor = line_vehicle_floor(rules, obs).unwrap_or(MIN_RESERVE_CREDITS);
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