//! What the production lines make: the house's target shape, and the type each
//! idle factory starts next.
//!
//! The old rule here ranked types by *summed* role weight, so one IFV's
//! anti-air, anti-infantry and anti-armour weights together answered a tank and
//! a conscript at once, `shortfall` came back empty, and the fallback — the
//! dearest type in the thinnest price band — bought the same IFV forever. That
//! is the "one type deep" army a play test reports. This module counts instead:
//! how many of the house's units can actually do each job
//! ([`Census::able`]) against how many of the enemy's units ask for it
//! ([`Census::threat`]), and it holds the infantry share to a floor, so the
//! barracks is a line that fires rather than one that stands idle.
//!
//! Every idle line is filled, including the aircraft and the naval ones: a
//! house that owns a factory of a category and never uses it has paid for
//! nothing.
//!
//! Determinism: integer comparisons over the rules' arrays in their own order,
//! with the role arithmetic (`f64`) used as a comparison only — the same
//! contract [`crate::rules::unit_role`] states for its weights.

use crate::rules::object_type::{ObjectCategory, ObjectType};
use crate::rules::ruleset::RuleSet;
use crate::rules::unit_role::{ROLES, Role};
use crate::sim::control::brain::Outbox;
use crate::sim::control::intent::Intent;
use crate::sim::control::observation::Observation;
use crate::sim::intern::InternedId;
use crate::sim::production::ProductionCategory;

use super::AssistBrain;
use super::build;
use super::census::{self, Census, Slot, price_tier};
use super::plan::Budget;

/// The wallet the house wants in hand before it buys troops. Below it every
/// credit buys another harvester or another bay, which is what makes the army
/// affordable later.
const COMBAT_MIN_CREDITS: i32 = 2_000;

/// The wallet the infantry line needs: a soldier costs a tenth of a tank, and
/// at the vehicle's floor the barracks never fires at all.
const INFANTRY_MIN_CREDITS: i32 = 500;

/// The wallet past which the house reaches for the heavy end; under it, one
/// line unit's price is as far as it goes ([`unit_ceiling`]).
const RICH_CREDITS: i32 = 4_000;

/// How many line vehicles in hand make a house rich enough that the infantry
/// ceiling stops applying, in vehicles.
///
/// The ceiling's reason is that the wallet has to reach one vehicle's price.
/// Six is "several times over": the house could buy a line vehicle and still
/// have the price of five more, so there is nothing left for the ceiling to
/// protect. Read against the line vehicle's own price rather than as a
/// constant, because the two sides' tanks cost different amounts.
const RICH_VEHICLES: i32 = 6;

/// The share of the fighting army that has to be infantry, in percent.
///
/// Infantry is the cheapest way to hold ground and the only answer to another
/// house's infantry that does not cost a factory's worth of credits per unit.
/// A brain that never buys it fights every engagement with vehicles it cannot
/// replace, which is what "still the IFV thing" describes from the outside.
const INFANTRY_SHARE_PERCENT: usize = 40;

/// The share of the fighting army infantry may *not* pass, in percent.
///
/// The floor above is what makes the barracks fire; this is what makes it
/// stop. A soldier is a tenth of a tank, so a barracks that answers every
/// frame buys infantry with every credit the house earns and the wallet never
/// reaches the price of a vehicle — the army a play test reports as "only ever
/// infantry". Past this the barracks waits for the vehicles to be paid for.
const INFANTRY_CEILING_PERCENT: usize = 55;

/// How much of one job's gap a single unit's weight can claim, in units. One
/// bottomless gap — a wall of conscripts — must not decide the whole army's
/// composition by itself.
const GAP_CAP: i32 = 3;

/// The most of one pool a single type may be, in percent, while another type
/// can still close a gap.
///
/// A gap the ranking answers is answered by *one* unit; the line that keeps
/// producing the same answer is a pool with one shape — the play test's "it
/// still only makes IFVs". Past this share the next best type takes the cycle.
const TYPE_SHARE_PERCENT: usize = 60;

/// The pool has to be big enough for a share to mean anything: two units of
/// three is 66%, and a factory told to rotate on the second vehicle of a
/// match makes a worse army, not a mixed one.
const TYPE_SHARE_MIN_POOL: usize = 5;

/// Fill every idle production line with what the house needs next.
pub(super) fn fill_lines(
    brain: &mut AssistBrain,
    rules: &RuleSet,
    obs: &Observation<'_>,
    budget: &Budget,
    out: &mut Outbox,
) {
    // What the yard is saving for is not the lines' money: it decided on a
    // structure, and paying for it is what "decided" means. Taken here, once,
    // so no line below can spend it.
    let mut budget = budget.reserving(brain.yard_reserve);
    brain.engineers.observe_outcomes(obs.tick(), brain.census.units.iter()
        .filter(|unit| unit.traits.engineer).count());
    // A house with a standing yard but no unloading bay must save its wallet
    // for the first refinery. The barracks' cheap infantry purchases could
    // otherwise keep its credits below the refinery's price indefinitely.
    // This never applies to recovering a *lost yard*: an existing factory
    // must still be permitted to buy the replacement MCV.
    if brain.director.recovering_income()
        && brain.census.yards > 0
        && brain.census.refineries == 0
        && !brain.director.under_attack_now()
    {
        return;
    }
    // The first miner is a hard dependency of every future purchase. Save
    // for its *actual rules price* and give the vehicle line first refusal;
    // otherwise an infantry-short opening can buy one cheap soldier every
    // frame and never accumulate the miner's price.
    let last_miner_needed = brain.director.recovering_income()
        && brain.census.refineries > 0
        && brain.census.harvesters == 0;
    if last_miner_needed
        && let Some((ty, _)) = build::harvester(rules, obs)
        && budget.free < obs.cost_of(rules, ty)
    {
        return;
    }
    // The infantry floor decides who gets the wallet first when it is thin: a
    // house below its share of infantry spends on the barracks before it
    // spends on the war factory.
    let (infantry_floor, infantry_ceiling) = infantry_targets(obs.side_index());
    let infantry_short = infantry_below_share(&brain.census, infantry_floor);
    // And the ceiling decides when it stops. A soldier costs a tenth of a tank,
    // so a barracks left to itself spends every credit the house earns on
    // infantry and the wallet never reaches the price of one — which is the
    // "only ever infantry" army. Past the ceiling the barracks waits, unless
    // the house is rich enough that the vehicles are not being starved.
    //
    // And only in the band of the wallet where a vehicle is actually
    // affordable. The ceiling is there to save the wallet for a tank; saving
    // it for a tank the line cannot yet pay for is a base producing nothing
    // at all — the barracks waiting on the vehicle's floor and the war factory
    // waiting on the same floor, both of them idle. So below that price the
    // barracks keeps its own line running, and at it the wallet is the war
    // factory's.
    //
    // The floor is one line vehicle's price, not a heavy unit's: held to the
    // dearer number the barracks spent every credit above its own floor and the
    // wallet never reached the price the war factory wanted — the army the
    // play test reports as "only infantry and dogs, no tank all match".
    let vehicle_floor =
        super::economy::line_vehicle_floor(rules, obs).unwrap_or(COMBAT_MIN_CREDITS);
    // A legal anti-battery unit may cost more than the current wallet.
    // Treat it as a real procurement commitment, not a soft recommendation:
    // otherwise cheaper infantry or line tanks continually consume its bank.
    let siege_bank = super::siege_counter::saving_for_counter(
        brain, rules, obs, budget.free);
    // Whether the barracks stops this frame.
    //
    // Two failure modes meet here, and the arm has to avoid both:
    //
    // - Gating the ceiling on `budget.free >= vehicle_floor` is a self-lock. A
    //   soldier costs a tenth of a tank, so a barracks willing to spend 200
    //   while the tank costs 700 keeps the wallet under 700 forever, and the
    //   ceiling that exists to make the wallet reach the tank never fires.
    //   Measured on Arena.mmx seed 0x5EED at 24000 ticks: `roster=[E1:48,
    //   MGTK:8, CAOILD:6]` — 48 riflemen, no tanks, war factory standing.
    // - Stopping unconditionally when the wallet is below the tank's price is
    //   the opposite failure: the barracks waits out its ceiling, the war
    //   factory waits out the price, and the base produces nothing at all.
    //
    // The arm that avoids both: past the ceiling the barracks stops *only*
    // when what it would spend is standing between the wallet and the tank.
    // One soldier under the price is a soldier that resets the saving; a
    // wallet that already holds the price is the war factory's turn and the
    // barracks stops regardless.
    let infantry_full = infantry_above_share(&brain.census, infantry_ceiling);
    let saving_for_the_tank = budget.free < vehicle_floor;
    let a_soldier_would_reset_the_saving =
        saving_for_the_tank && budget.free >= vehicle_floor - INFANTRY_MIN_CREDITS;
    // A house holding many vehicles' worth of credits is not saving for a
    // vehicle; it may recruit a useful infantry screen in parallel. The old
    // policy exempted *every* rich barracks cycle from the infantry ceiling,
    // however, and long rich games accumulated an army of mostly riflemen.
    // After a small screen and a reasonable infantry share, prioritise the
    // armoured line, advanced technology and specialist recruitment instead.
    //
    // Measured at `--credits 100000`, Arena.mmx seed 0x5EED, 30000 ticks:
    // `infantry=72 / fighters=87` — 83% of the army was infantry, so the
    // 55% ceiling had stopped the barracks long before; the vehicle line was
    // one factory building one unit at a time; and 38306 credits sat unspent
    // at the end of the match while income ran at 180 a minute. The army the
    // money could have bought was never bought.
    //
    // `RICH_VEHICLES` is the same intent the fixed `RICH_CREDITS` above
    // carries, read against the price that matters instead of a constant: six
    // line vehicles in hand. At 2000 credits of free cash (the ordinary match)
    // this is false, so the ceiling keeps its documented behaviour; at 100000
    // it is true from the first frame; the infantry mix still gets a cap.
    let rich = budget.free >= vehicle_floor.saturating_mul(RICH_VEHICLES);
    let infantry: usize = brain.census.tiers[Slot::Infantry.index()].iter().sum();
    let vehicles: usize = brain.census.tiers[Slot::Vehicle.index()].iter().sum();
    // More money means we can build both branches, not that an unchecked
    // barracks may swamp every future tank with low-cost infantry. At least
    // six infantry may always form the first screen; after that hold if the
    // line armour is still less than half the infantry count.
    let rich_needs_armour = !brain.director.under_attack_now() && rich
        && (infantry >= 6 && infantry_full && vehicles.saturating_mul(2) < infantry
            || rich_infantry_saturated(infantry, brain.census.fighters));
    // During the opening, preserve cash for the first armored wave and the
    // lab. Percentages alone permit an army of twenty cheap infantry while
    // the war factory and research queue cannot be funded. Keep a small
    // home screen, but stop mass production until the mining economy works.
    let infantry: usize = brain.census.tiers[Slot::Infantry.index()].iter().sum();
    let fragile_opening = brain.census.refineries <= 1
        && infantry >= 5 && budget.free < vehicle_floor.saturating_mul(3)
        && !brain.director.under_attack_now();
    let barracks_holds = fragile_opening || rich_needs_armour
        || (infantry_full && !rich
            && (budget.free >= vehicle_floor || a_soldier_would_reset_the_saving));
    // Why the infantry line kept running: the ceiling is only as good as the
    // three numbers it reads. Traced like the build list's own line, so a
    // roster that is four fifths infantry can be attributed instead of guessed.
    if super::trace_enabled("AUTOPLAY_TRACE_COMPOSITION") && obs.tick() % 1800 == 0 {
        let infantry: usize = brain.census.tiers[Slot::Infantry.index()].iter().sum();
        log::info!(
            "composition trace: infantry={} fighters={} free={} vehicle_floor={} rich={} full={} below={} holds={} resetting={}",
            infantry,
            brain.census.fighters,
            budget.free,
            vehicle_floor,
            rich,
            infantry_full,
            infantry_short,
            barracks_holds,
            a_soldier_would_reset_the_saving,
        );
    }
    let airport_count = obs.own_entities().filter(|entity| {
        obs.object_type(rules, entity.type_id).is_some_and(|ty| {
            ty.factory_type() == Some(crate::rules::object_type::FactoryType::AircraftType)
        })
    }).count();
    let aircraft_count: usize = brain.census.tiers[Slot::Aircraft.index()].iter().sum();
    let establish_air_cover = airport_count > 0 && vehicles >= 4
        && aircraft_count < airport_count.saturating_mul(2)
        && !brain.director.recovering_income()
        && brain.census.refineries > 0
        && brain.census.harvesters > 0;
    let order = if last_miner_needed {
        [
            ProductionCategory::Vehicle,
            ProductionCategory::Infantry,
            ProductionCategory::Aircraft,
            ProductionCategory::Ship,
        ]
    } else if super::siege_counter::prefers_air_line(brain,rules,obs)
        || establish_air_cover && budget.free >= COMBAT_MIN_CREDITS {
        // Fill at most two aircraft per airfield before resuming the usual
        // tank-first spending. This is a one-time investment in air coverage,
        // not a permanent rule that starves the ground army.
        [
            ProductionCategory::Aircraft,
            ProductionCategory::Vehicle,
            ProductionCategory::Infantry,
            ProductionCategory::Ship,
        ]
    } else if infantry_short {
        [
            ProductionCategory::Infantry,
            ProductionCategory::Vehicle,
            ProductionCategory::Aircraft,
            ProductionCategory::Ship,
        ]
    } else {
        [
            ProductionCategory::Vehicle,
            ProductionCategory::Infantry,
            ProductionCategory::Aircraft,
            ProductionCategory::Ship,
        ]
    };
    for category in order {
        if siege_bank && !brain.director.under_attack_now() {
            // Economically functional bases retain the money to buy the
            // requested counter when its native price is reached. Emergency
            // home defence and restoration of the last miner run above this.
            continue;
        }
        // An army that already holds its share of infantry stops buying it,
        // so the wallet can reach a vehicle — unless the wallet is so far under
        // that price that one soldier would not set the saving back. Then the
        // barracks keeps turning rather than leaving both lines idle.
        // Savings forbid another mass rifleman purchase, not a single spy
        // against an actually observed infiltration opportunity. Otherwise
        // the opening's five-infantry cap silently blocks every spy when a
        // one-refinery house most needs enemy cash.
        let urgent_infiltration = category == ProductionCategory::Infantry
            && !brain.director.under_attack_now()
            && !brain.director.recovering_income()
            && early_spy_infiltration(brain, rules, obs, budget.free).is_some();
        let urgent_capture = category == ProductionCategory::Infantry
            && (neutral_asset_engineer(brain, rules, obs, budget.free).is_some()
                || urgent_engineer(brain, rules, obs, budget.free).is_some()
                || maintenance_engineer(brain, rules, obs, budget.free).is_some());
        // A useful empty IFV or open-top fortress has a genuine capability
        // gap when no existing spare soldier can fill its fire port. Permit
        // *one* rules-derived passenger purchase even if generic infantry
        // share has reached its ceiling, but not over the strategic lab bank.
        let combo_due = category == ProductionCategory::Infantry
            && combo_passenger_purchase(brain, rules, obs, budget.free).is_some();
        // The wallet saved for a scarce-map lab should not be silently spent
        // by ordinary unit ranking, but an engineer or spy with a real mission
        // may still use its own specialist priority.
        let bank_for_tech = save_for_tech(brain, rules, obs, &budget);
        // A *visible, active* heavy battery is an urgent production objective.
        // The old saving gates skipped the vehicle and air lines completely
        // and then purchased more cheap infantry while the army died at the
        // tower. Let a native-buildable counter compete with ordinary saving;
        // the yard's actual queued/committed money was already reserved above.
        let siege_due = matches!(category,
            ProductionCategory::Vehicle | ProductionCategory::Aircraft)
            && super::siege_counter::purchase(
                brain, rules, obs, category, budget.free).is_some();
        let heavy_cost = scarce_advanced_vehicle(brain,rules,obs,budget.income)
            .map(|(ty,_)|obs.cost_of(rules,ty));
        let bank_for_heavy = heavy_cost.is_some_and(|cost| budget.free < cost);
        if (bank_for_tech || bank_for_heavy)
            && !urgent_infiltration && !urgent_capture && !siege_due { continue; }
        if barracks_holds && category == ProductionCategory::Infantry
            && !urgent_infiltration && !urgent_capture && !combo_due
        { continue; }
        // A line holding a product, a queue or a finished object is working and
        // is not given another request; a line with nothing in it — including
        // one the house has not used yet — can be started. Whether the house
        // can make any type of this category at all is `CanBuild`'s answer, and
        // the candidate list below is already filtered by it.
        if brain.census.line(category).busy() {
            continue;
        }
        // A line the house does not own is not idle, it is absent: an aircraft
        // type the prerequisites allow but no airfield can make answers `Yes`
        // to `CanBuild` and is refused at the queue, every frame, for as long
        // as the wallet stays above its price. The plant is read off the rules
        // (`Factory=AircraftType`), which is the same owner the build list uses.
        if category == ProductionCategory::Aircraft && !has_aircraft_factory(rules, obs) {
            continue;
        }
        if category == ProductionCategory::Ship && !has_naval_factory(rules, obs) {
            continue;
        }
        let wanted = product_for(
            brain,
            rules,
            obs,
            &budget,
            category,
            infantry_short,
            vehicle_floor,
        );
        // Only a request the shared ledger accepted counts as purchased.
        // The local budget is mutable: each category sees the credits already
        // promised to every earlier category *and* to the yard this frame.
        let queued = queue_unit(rules, obs, &mut budget, out, wanted);
        if queued && let Some((ty, _)) = wanted {
            let traits = census::Traits::of(rules, ty);
            if traits.engineer {
                brain.engineers.note_bought(obs.tick());
            } else if traits.spy {
                // Infiltration is neither an engineer purchase nor an
                // immediate capture. The ledger for failed captures must not
                // silence the spy line, and spies need their own pacing.
                brain.next_spy_purchase = obs.tick().saturating_add(
                    if obs.credits() >= 5_000 { 450 } else { 900 });
            }
        }
    }
}

/// Whether the army is short of its infantry share.
///
/// A house with no army at all is short of it: `fighters.max(1)` makes the
/// first soldier the floor's, rather than leaving an empty army's share at
/// zero and letting the war factory decide what the opening is made of.
fn infantry_below_floor(census: &Census) -> bool {
    infantry_below_share(census, INFANTRY_SHARE_PERCENT)
}

fn infantry_above_ceiling(census: &Census) -> bool {
    infantry_above_share(census, INFANTRY_CEILING_PERCENT)
}

/// The common 40/55 infantry band overproduces cheap Soviet infantry while
/// its war factory waits for Rhino/Apocalypse money. Yuri's mind-control
/// infantry is more valuable than a conscript, so its ceiling remains higher.
/// The side numbers come from the rules' country side index, not unit names.
/// Wealth does not make low-tier riflemen infinitely useful. Once a screen
/// exists, leave the next barracks cycle to armour, tech and specialist tools
/// instead of treating a six-vehicle wallet as unlimited infantry permission.
fn rich_infantry_saturated(infantry: usize, fighters: usize) -> bool {
    infantry >= 8 && infantry.saturating_mul(100) >= fighters.max(1).saturating_mul(35)
}

fn infantry_targets(side: Option<i32>) -> (usize, usize) {
    match side {
        Some(1) => (25, 42), // Soviet: keep vehicle money in the war factory
        Some(2) => (35, 52), // Yuri: retain room for control infantry
        _ => (INFANTRY_SHARE_PERCENT, INFANTRY_CEILING_PERCENT),
    }
}

fn infantry_below_share(census: &Census, percent: usize) -> bool {
    let infantry: usize = census.tiers[Slot::Infantry.index()].iter().sum();
    infantry * 100 < census.fighters.max(1) * percent
}

fn infantry_above_share(census: &Census, percent: usize) -> bool {
    let infantry: usize = census.tiers[Slot::Infantry.index()].iter().sum();
    infantry * 100 > census.fighters.max(1) * percent
}

/// How many engineers the house keeps, and how many spies.
///
/// Two and one. An engineer is spent the moment it walks into a building, so
/// the house wants another behind it — a captured refinery is worth a dozen
/// soldiers, and the second engineer is the one that takes it after the first
/// is gone. A spy is a raid that walks for minutes and is worth exactly one at
/// a time: it does its work in a single building and does not come out.
const ENGINEER_WANT: usize = 2;
// Three, not one. The user's report: "AI 不太会分辨间谍，偷钱几乎成功率很高" —
// the enemy does not screen for a spy, so the walk usually lands. At one the
// house could ever take one infiltration per match, and the veteran classes
// (barracks, war factory, lab) are three separate prizes.
const SPY_WANT: usize = 3;

/// How many attack dogs the house keeps: one for the scan, a second behind it
/// for the dog the enemy sends at the first.
const DOG_WANT: usize = 2;
/// Crates can spawn anywhere. Once the normal scouts and infiltration tools
/// are established, a prosperous house may keep a larger independent crate
/// detail without making the opening starve for dogs.
const CRATE_SCOUT_WANT: usize = 4;

/// Engineers that may come to nothing before the house stops buying them.
///
/// An engineer is consumed by success and by failure alike, so a live count
/// cannot tell the two apart: buy two, take two buildings, and the count reads
/// zero and asks for two more. That is the wishlist's 狂造工程师拖垮经济 — the
/// economy dies of a good idea repeated past the point where there is anything
/// worth taking.
///
/// A capture is worth roughly a refinery, so a house that takes one building
/// every two engineers is doing well and should keep buying. The number here is
/// the point where the errands are not landing: four bought per building taken.
const ENGINEER_WASTE_PATIENCE: u32 = 4;

/// How long the house stops buying engineers after that ratio is passed, in
/// frames: two minutes. Long enough for the front to move and the remaining
/// neutral buildings to be taken or lost, short enough to try again.
const ENGINEER_QUIET_FRAMES: u64 = 1_800;

/// What the engineer line has spent, and what it got for it.
///
/// The one piece of state this file keeps: a count of units produced is not a
/// ledger, and the question "is another engineer worth buying" is a ratio of
/// two totals rather than a reading of the field.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EngineerLedger {
    /// Engineers bought, as the queue saw them. Spies have a separate cooldown.
    pub(super) bought: u32,
    /// Buildings taken: a capture that landed.
    pub(super) captured: u32,
    /// The frame the house stops buying until, once the ratio fails.
    pub(super) quiet_until: u64,
    /// Failures already considered. A living engineer is not yet a loss.
    accounted_failures: u32,
    /// Limit retries in an active siege even when the normal loss ratio cools down.
    last_bought_at: Option<u64>,
}

impl EngineerLedger {
    /// Whether the house may buy another engineer this frame.
    ///
    /// The window is the only gate. The ratio decides whether a purchase
    /// *enters* a window, not whether the house may ever buy again: a ratio
    /// that failed stays failed as the count grows, so gating on it directly
    /// would stop the line for the rest of the match after one bad run. What
    /// the house does instead is try one more when the window expires — if that
    /// one lands, the ratio recovers; if not, it waits another window.
    pub(super) fn may_buy(&self, tick: u64) -> bool {
        tick >= self.quiet_until
    }

    /// Record one bought.
    pub(super) fn note_bought(&mut self, tick: u64) {
        self.bought = self.bought.saturating_add(1);
        self.last_bought_at = Some(tick);
    }

    fn siege_retry_ready(&self, tick: u64) -> bool {
        self.last_bought_at.is_none_or(|last|
            tick.saturating_sub(last) >= 600)
    }

    /// Only *lost* engineers count as a failed capture. The old buy/capture
    /// ratio punished freshly purchased, still-walking engineers as failures
    /// and could freeze a perfectly safe oil-derrick expansion for two minutes.
    pub(super) fn observe_outcomes(&mut self, tick: u64, alive: usize) {
        let lost = self.bought.saturating_sub(self.captured)
            .saturating_sub(u32::try_from(alive).unwrap_or(u32::MAX));
        if lost >= ENGINEER_WASTE_PATIENCE
            && lost > self.captured.saturating_mul(2)
            && lost > self.accounted_failures
        {
            self.quiet_until = tick.saturating_add(ENGINEER_QUIET_FRAMES);
            self.accounted_failures = lost;
        }
    }

    /// Record one building taken by a capturer.
    pub(super) fn note_captured(&mut self) {
        self.captured = self.captured.saturating_add(1);
    }
}

/// The tool this line should make, when the house has fewer than it wants and
/// can pay for one.
///
/// Neither an engineer nor a spy is army, and neither was being made: the
/// house's own choice (`chosen_product`) reads the AI's unit choices, which
/// the simulation never fills for a house a human controls, and the army's own
/// ranking refuses both. So the capture and infiltration steps have been
/// running against an empty army — the engineer that was never built.
fn tool<'r>(
    rules: &'r RuleSet,
    obs: &Observation<'_>,
    census: &Census,
    credits: i32,
    ledger: &EngineerLedger,
    spy_ready: bool,
    known_spy_site: bool,
    crate_work: bool,
) -> Option<(&'r ObjectType, InternedId)> {
    let engineers = census
        .units
        .iter()
        .filter(|unit| unit.traits.engineer)
        .count();
    let spies = census.units.iter().filter(|unit| unit.traits.spy).count();
    // The attack dog: `NotHuman=yes` with `Natural=yes` is retail's only such
    // pair, and it is the unit the scan role is built on. Nothing else in the
    // infantry list can look at the map — a rifleman walks where it is put and
    // sees only what its gun reaches.
    let dogs = census.units.iter().filter(|unit| unit.traits.scout).count();
    // The live count is what the house has; the ledger is what it has spent.
    // Both are needed: the count says whether there is a hole, the ledger says
    // whether filling it has been working.
    let may_buy_capturers = ledger.may_buy(obs.tick());
    // The opening needs two engineers, but a prosperous house with several
    // *discovered, locally clear* capturable sites may support parallel
    // operations. Unseen enemy technology is never counted as opportunity.
    let clear_sites = obs.capturable_buildings(rules).filter(|target| {
        obs.is_cell_revealed(target.cell.0, target.cell.1)
            && (!obs.is_hostile(target.entity_id)
                || obs.is_cell_visible(target.cell.0, target.cell.1))
            && !obs.visible_enemy_entities().any(|enemy| {
                obs.object_type(rules, enemy.type_id)
                    .is_some_and(|ty| ty.category != ObjectCategory::Building)
                    && super::squared_distance(enemy.cell, target.cell) <= 7 * 7
            })
    }).take(12).count();
    // Generated maps can hold six or more genuinely capturable abandoned
    // factories and yards. A two-engineer ceiling leaves them standing while
    // the opposing side develops. Scale only with *revealed, locally safe*
    // opportunities and cash, never with omniscient hidden neutral sites.
    let engineer_want = if clear_sites >= 6 && credits >= 2_500 { 6 }
        else if clear_sites >= 3 && credits >= 1_500 { 4 }
        else if clear_sites >= 2 && credits >= 900 { 3 }
        else { ENGINEER_WANT };
    let want_engineer = engineers < engineer_want && may_buy_capturers;
    let visible_spy_target = obs.spyable_buildings(rules).any(|target| {
        obs.is_cell_visible(target.cell.0, target.cell.1)
    });
    // An identified infiltration target gets priority, but a wealthy house
    // that already owns its scouting/capture tools can prepare one spy before
    // its first sighting. A hard visibility-only requirement prevented both
    // the existing tool fixture and real fog-of-war anticipation from working.
    let spy_preparation = engineers >= ENGINEER_WANT
        && dogs >= DOG_WANT
        && credits >= 3_000;
    // A dog sighting remains useful for a later spy even after fog closes.
    // The site authorizes *reconnaissance only*, never a blind Capture click.
    let want_spy = spies < SPY_WANT && spy_ready
        && (visible_spy_target || (known_spy_site && credits >= 900)
            || spy_preparation);
    // Once the first scout and engineer are available, a visible infiltration
    // opportunity deserves a slot before another engineer is purchased.
    let urgent_spy = want_spy && dogs >= 1 && engineers >= 1 && spies == 0;
    let want_dog = dogs < DOG_WANT
        || (crate_work && dogs < CRATE_SCOUT_WANT
            && spies >= 1 && credits >= 2_500);
    if !want_engineer && !want_spy && !want_dog {
        return None;
    }
    build::class_types(rules, obs, ObjectCategory::Infantry)
        .map(|(ty, id)| (ty, id, census::Traits::of(rules, ty)))
        .filter(|(_, _, traits)| {
            (traits.engineer && want_engineer)
                || (traits.spy && want_spy)
                || (traits.scout && want_dog)
        })
        .filter(|(ty, _, _)| obs.cost_of(rules, ty) <= credits)
        // The dog first. It is the cheapest pair of eyes the house has, it wins
        // the engineer race against a house that opens without one, and it is
        // the only early answer to an enemy engineer walking into the base —
        // without one the house loses the capture race it never noticed it was
        // in. The engineer second: a spy's work is a raid, an engineer's is the
        // base's own growth. Then the cheaper, then the earlier rule.
        .min_by_key(|(ty, id, traits)| {
            (
                u8::from(!(urgent_spy && traits.spy)),
                u8::from(!traits.scout),
                u8::from(!traits.engineer),
                obs.cost_of(rules, ty),
                *id,
            )
        })
        .map(|(ty, id, _)| (ty, id))
}

/// Buy a small, bounded capture detail for **currently visible** neutral
/// tech, abandoned yards, production plants and public infrastructure. The
/// native engineer click still determines capture legality on arrival. We do
/// not consult shrouded neutral structures to plan spending or navigation.
fn neutral_asset_engineer<'r>(
    brain: &AssistBrain, rules: &'r RuleSet, obs: &Observation<'_>, free: i32,
) -> Option<(&'r ObjectType, InternedId)> {
    // A large starting wallet or captured oil field can fund engineers
    // before a conventional refinery exists; do not require harvesters when
    // the purpose is unlocking an abandoned technology tree.
    if free < 1_500 || (brain.census.refineries == 0 && free < 10_000)
        || brain.director.under_attack_now() || brain.director.recovering_income()
        || !brain.engineers.may_buy(obs.tick())
    { return None; }
    let objectives = obs.neutral_structures()
        .filter(|site| obs.is_cell_visible(site.cell.0, site.cell.1))
        .filter(|site| obs.object_type(rules, site.type_id).is_some_and(|ty|
            ty.capturable && (ty.build_const_eligible()
                || ty.factory_type().is_some()
                || rules.build_tech_types.iter().any(|name| *name == ty.id)
                || super::orders::strategic_public_asset(ty))))
        .filter(|site| !obs.visible_enemy_entities().any(|enemy|
            super::squared_distance(enemy.cell, site.cell) <= 8 * 8))
        .take(12).count();
    // V37 keeps a short, visible-object mission queue across production
    // frames. A tech prize must not lose its engineer slot simply because
    // ordinary production ran one frame before `orders::captures`.
    let operations = brain.asset_ops.visible_opportunities(obs);
    let objectives = objectives.max(operations);
    if objectives == 0 { return None; }
    let engineers = brain.census.units.iter().filter(|unit| unit.traits.engineer).count();
    if engineers >= objectives.saturating_add(1).clamp(2, 4) { return None; }
    build::class_types(rules, obs, ObjectCategory::Infantry)
        .filter(|(ty, _)| census::Traits::of(rules, ty).engineer)
        .filter(|(ty, _)| obs.cost_of(rules, ty) <= free)
        .min_by_key(|(ty, id)| (obs.cost_of(rules, ty), *id))
}

/// A siege without any engineer can surround a capturable yard for minutes
/// while the ordinary tank/infantry ranking spends the entire wallet.
/// Use only currently visible, *legally capturable* enemy facilities and a
/// real nearby friendly force; a hidden yard never authorizes a purchase.
fn urgent_engineer<'r>(
    brain: &AssistBrain, rules: &'r RuleSet, obs: &Observation<'_>,
    free: i32,
) -> Option<(&'r ObjectType, InternedId)> {
    if brain.director.recovering_income() || brain.director.under_attack_now()
        || brain.census.refineries == 0 || brain.census.harvesters == 0
    { return None; }
    let live = brain.census.units.iter().filter(|unit|
        unit.traits.engineer && !unit.in_service()).count();
    let target_count = if free >= 3_500 { 2 } else { 1 };
    if live >= target_count || !brain.engineers.siege_retry_ready(obs.tick()) {
        return None;
    }
    let siege = obs.capturable_buildings(rules).any(|target| {
        obs.is_hostile(target.entity_id)
            && obs.is_cell_visible(target.cell.0, target.cell.1)
            && obs.object_type(rules, target.type_id).is_some_and(|ty|
                ty.construction_yard || ty.factory_type().is_some()
                    || super::orders::strategic_public_asset(ty))
            && brain.census.units.iter().filter(|unit|
                unit.controllable() && unit.traits.combat
                    && super::squared_distance(unit.cell, target.cell) <= 24 * 24
            ).take(4).count() >= 4
    });
    // A bridge already inspected by the player is also a valid engineer job.
    // It must be near an active base or friendly force, not a hidden hut at
    // the opposite end of the map.
    let bridge = obs.bridge_repair_huts(rules).any(|hut| {
        obs.is_cell_visible(hut.cell.0, hut.cell.1)
            && brain.census.units.iter().any(|unit|
                unit.controllable()
                    && super::squared_distance(unit.cell, hut.cell) <= 35 * 35)
            && !obs.visible_enemy_entities().any(|enemy|
                super::squared_distance(enemy.cell, hut.cell) <= 8 * 8)
    });
    if !siege && !bridge { return None; }
    build::class_types(rules, obs, ObjectCategory::Infantry)
        .filter(|(ty, _)| census::Traits::of(rules, ty).engineer)
        .filter(|(ty, _)| obs.cost_of(rules, ty) <= free)
        .min_by_key(|(ty, id)| (obs.cost_of(rules, ty), *id))
}

/// Engineers are the IFV repair weapon, not regular riflemen. An empty
/// Gunner chassis without a spare engineer must trigger recruitment before
/// the normal infantry composition can consume the wallet. This query uses
/// the actual IFVMode to avoid producing an unsupported mechanic in mods.
fn maintenance_engineer<'r>(
    brain: &AssistBrain, rules: &'r RuleSet,
    obs: &Observation<'_>, free: i32,
) -> Option<(&'r ObjectType, InternedId)> {
    let engineers = brain.census.units.iter()
        .filter(|unit| unit.traits.engineer && !unit.in_service()).count();
    let tanks = brain.census.units.iter().filter(|unit|
        unit.traits.combat && obs.entity_type(rules, unit.entity_id)
            .is_some_and(|ty| ty.category == ObjectCategory::Vehicle && !ty.naval)).count();
    if engineers >= 2 || tanks < 5 || free < 1_000
        || brain.census.refineries < 2 || brain.census.harvesters < 2
        || brain.director.recovering_income() || brain.director.under_attack_now()
        || !brain.engineers.siege_retry_ready(obs.tick())
    { return None; }
    let exists_gunner = brain.census.units.iter().any(|unit| {
        obs.entity_type(rules, unit.entity_id).is_some_and(|ty|
            ty.gunner && ty.passengers > 0)
    });
    if !exists_gunner || brain.census.units.iter().any(|unit|
        super::advanced::repair_ifv_loaded(rules, obs, &brain.census, unit.entity_id)) {
        return None;
    }
    build::class_types(rules, obs, ObjectCategory::Infantry)
        .filter(|(ty, _)| census::Traits::of(rules, ty).engineer)
        .filter(|(ty, _)| obs.cost_of(rules, ty) <= free)
        // A gunner IFV already filled with a GI is still convertible:
        // free_repair_ifv_seat will ask it to Unload once two engineers exist.
        .filter(|(engineer, _)| brain.census.units.iter().any(|unit|
            obs.entity_type(rules, unit.entity_id).is_some_and(|carrier|
                carrier.gunner && super::advanced::gunner_repairs(
                    rules, carrier, engineer.ifv_mode))))
        .min_by_key(|(ty,id)| (obs.cost_of(rules,ty), *id))
}

/// Small ore fields cannot replace endless low-tier casualties. Once a
/// defensible army and a working miner exist, protect the next genuine lab
/// or radar purchase rather than buying one cheap infantry every tick.
fn save_for_tech(
    brain: &AssistBrain, rules: &RuleSet,
    obs: &Observation<'_>, budget: &Budget,
) -> bool {
    if obs.tick() < 3_000 || brain.director.under_attack_now()
        || brain.director.recovering_income() || brain.census.fighters < 12
        || brain.census.refineries == 0 || brain.census.harvesters == 0
        || build::count_in(obs, rules, &brain.census, &rules.build_tech_types) > 0
        || !budget.income.is_some_and(|income| income > 0 && income <= 700)
        || brain.ore_known.len() >= 18
    { return false; }
    let next = build::tech_building(rules, obs).or_else(||
        build::first_buildable(rules, obs, &rules.build_radar_types));
    next.is_some_and(|(ty,_)| budget.free < obs.cost_of(rules,ty))
}

/// The high-tech combat unit the already-built laboratory has made
/// *actually* legal. On a thin ore field an endless cheap infantry queue is a
/// debt against one genuine heavy tank. Choose from the native buildable
/// roster, never a guessed faction name or an unavailable prerequisite.
fn scarce_advanced_vehicle<'r>(
    brain: &AssistBrain, rules: &'r RuleSet,
    obs: &Observation<'_>, income: Option<i32>,
) -> Option<(&'r ObjectType, InternedId)> {
    if obs.tick() < 3_000 || brain.director.under_attack_now()
        || brain.director.recovering_income() || brain.census.fighters < 12
        || brain.census.refineries == 0 || brain.census.harvesters == 0
        || build::count_in(obs,rules,&brain.census,&rules.build_tech_types) == 0
        || brain.ore_known.len() >= 18
        || !income.is_some_and(|income| income > 0 && income <= 800)
    { return None; }
    build::class_types(rules,obs,ObjectCategory::Vehicle)
        .filter(|(ty,_)| !ty.naval && !ty.resource_gatherer
            && census::Traits::of(rules,ty).combat
            && super::doctrine::may_produce(brain,rules,obs,ty))
        .filter(|(ty,_)| (1_500..=3_400).contains(&obs.cost_of(rules,ty)))
        .min_by_key(|(ty,id)| (
            brain.census.type_count(*id),
            std::cmp::Reverse(obs.cost_of(rules,ty)), *id,
        ))
}

/// Only a prosperous army with a visible, distant capture objective asks
/// for one specialized carrier. Ordinary combat cannot be starved by a fleet
/// of empty transports; the shuttle state machine remains optional.
fn expedition_carrier<'r>(
    brain: &AssistBrain, rules: &'r RuleSet, obs: &Observation<'_>,
    category: ProductionCategory, free: i32,
) -> Option<(&'r ObjectType, InternedId)> {
    if obs.tick() < 4_500 || free < 9_000
        || brain.director.under_attack_now() || brain.director.recovering_income()
        || brain.census.refineries < 2 || brain.census.harvesters < 2
        || brain.census.fighters < 12
        || !brain.census.units.iter().any(|unit| unit.traits.engineer
            && unit.controllable() && !unit.in_service())
        || brain.census.units.iter().any(|unit| obs.entity_type(rules,unit.entity_id)
            .is_some_and(|ty| ty.passengers > 0 && !ty.gunner && !ty.open_topped
                && matches!(ty.movement_zone,
                    crate::rules::locomotor_type::MovementZone::Fly
                    | crate::rules::locomotor_type::MovementZone::Water
                    | crate::rules::locomotor_type::MovementZone::WaterBeach)
                && super::pilots::Pilot::of(rules,ty)==super::pilots::Pilot::Transport))
    { return None; }
    let distant = obs.capturable_buildings(rules).any(|site| {
        obs.is_cell_revealed(site.cell.0,site.cell.1)
            && (!obs.is_hostile(site.entity_id)
                || obs.is_cell_visible(site.cell.0,site.cell.1))
            && brain.census.units.iter().any(|unit| unit.traits.engineer
                && super::squared_distance(unit.cell,site.cell)>=45*45)
    });
    if !distant { return None; }
    let (object_category,naval) = match category {
        ProductionCategory::Aircraft => (ObjectCategory::Aircraft,false),
        ProductionCategory::Ship => (ObjectCategory::Vehicle,true),
        ProductionCategory::Vehicle => (ObjectCategory::Vehicle,false),
        _ => return None,
    };
    build::class_types(rules,obs,object_category)
        .filter(|(ty,_)| ty.naval==naval && ty.passengers>0
            && !ty.gunner && !ty.open_topped
            && super::pilots::Pilot::of(rules,ty)==super::pilots::Pilot::Transport
            && matches!(ty.movement_zone,
                crate::rules::locomotor_type::MovementZone::Fly
                | crate::rules::locomotor_type::MovementZone::Water
                | crate::rules::locomotor_type::MovementZone::WaterBeach))
        .filter(|(ty,_)| obs.cost_of(rules,ty)<=free.saturating_sub(6_000))
        .min_by_key(|(ty,id)|(obs.cost_of(rules,ty),*id))
}


/// Fill one *currently empty* combat carrier by manufacturing a genuine
/// rules-compatible passenger, only after the economy can afford a luxury
/// combined platform. Do not buy a passenger while there is already a
/// suitable, free soldier who can board it. The carrier itself and the
/// soldier are found from actual ownership and current `CanBuild`; this also
/// discovers cross-faction combinations after acquiring another tech tree.
fn combo_passenger_purchase<'r>(
    brain: &AssistBrain, rules: &'r RuleSet, obs: &Observation<'_>, free: i32,
) -> Option<(&'r ObjectType, InternedId)> {
    if brain.director.under_attack_now() || brain.director.recovering_income()
        || brain.census.refineries < 2 || brain.census.harvesters < 2
        || brain.census.fighters < 12 || free < 4_000
    { return None; }
    let available_defenders = brain.squads.squad(super::squads::Role::Defend)
        .map(|squad| &squad.members);
    let carrier = brain.census.units.iter()
        .filter(|u| u.controllable() && !u.in_service())
        .filter_map(|u| {
            let ty = obs.entity_type(rules, u.entity_id)?;
            ((ty.gunner || ty.open_topped) && ty.passengers > 0
                && obs.passengers(u.entity_id) < ty.passengers as usize
                && !ty.naval
                && brain.census.base_cell.is_some_and(|home|
                    super::squared_distance(home, u.cell) <= 24 * 24))
                .then_some((u, ty))
        })
        .find(|(carrier,ty)| !brain.census.units.iter().any(|soldier| {
            if !soldier.controllable() || soldier.in_service()
                || !available_defenders.is_some_and(|members|
                    members.contains(&soldier.entity_id))
                || soldier.slot != Slot::Infantry || !soldier.traits.combat
                || obs.inside_transport(soldier.entity_id).is_some()
                || super::squared_distance(soldier.cell, carrier.cell) > 24 * 24
                || !obs.can_enter_transport(rules, soldier.entity_id, carrier.entity_id)
            { return false; }
            obs.entity_type(rules, soldier.entity_id).is_some_and(|kind|
                super::combinations::passenger_value(rules,obs,ty,kind,carrier.cell) > 0)
        }))?;
    build::class_types(rules, obs, ObjectCategory::Infantry)
        .filter(|(ty,_)| super::orders::is_combat_unit(rules,ty)
            && !ty.engineer && !ty.agent && !ty.infiltrate
            && !ty.c4 && !ty.naval)
        .filter(|(ty,_)| obs.cost_of(rules,ty) <= free.saturating_sub(2_000))
        .filter_map(|(ty,id)| {
            let value=super::combinations::passenger_value(
                rules,obs,carrier.1,ty,carrier.0.cell);
            (value > 0).then_some((ty,id,value,obs.cost_of(rules,ty)))
        })
        .max_by_key(|(_,id,value,cost)| (
            value.saturating_mul(1_000) / i64::from((*cost).max(1)),
            *value, std::cmp::Reverse(*id),
        ))
        .map(|(ty,id,_,_)| (ty,id))
}

/// What one idle line starts next.
fn product_for<'r>(
    brain: &mut AssistBrain,
    rules: &'r RuleSet,
    obs: &Observation<'_>,
    budget: &Budget,
    category: ProductionCategory,
    infantry_short: bool,
    vehicle_floor: i32,
) -> Option<(&'r ObjectType, InternedId)> {
    // The house's own choice first: `HouseClass::Suggest_New_Object
    // 0x004FBD80` is where the native team data reaches production, and a team
    // that asks for an engineer or a spy is answered here — no ranking over
    // combat weights can ask for those at all. It is unchecked (§ the reader),
    // so the wallet is this caller's guard.
    // Recover the construction yard before any native product preference:
    // without a yard the economy cannot replace buildings at all. A lost
    // yard is a different job from buying an optional second MCV and must
    // never be blocked by the strategic expansion gate.
    if category == ProductionCategory::Vehicle
        && !super::expand::yard_stands(&brain.census)
        && !super::expand::owns_yard_maker(&brain.census)
        && let Some(maker) = super::expand::yard_maker(rules, obs)
    {
        return Some(maker);
    }
    // Restore the *last* harvester before satisfying native team wishes.
    // A chosen combat vehicle can be cheaper and would otherwise keep the
    // wallet below the price of the sole miner that restores future income.
    if category == ProductionCategory::Vehicle
        && brain.director.recovering_income()
        && brain.census.refineries > 0
        && brain.census.harvesters == 0
        && let Some(found) = build::harvester(rules, obs)
    {
        return Some(found);
    }
    // A damaged tank force needs an actual repair IFV chassis, not just an
    // engineer that would board one if it happened to exist. Limit this to
    // *one* Gunner vehicle, and only when a spare engineer's native IFVMode
    // selects a negative-damage repair weapon under the current mod rules.
    if category == ProductionCategory::Vehicle
        && !brain.director.recovering_income()
        && !brain.director.under_attack_now()
        && brain.census.refineries > 0 && brain.census.harvesters >= 2
        && budget.free >= 3_500
        && brain.census.units.iter().filter(|unit| unit.traits.engineer
            && unit.controllable()).count() >= 1
        && brain.census.units.iter().filter(|unit| unit.traits.combat
            && obs.entity_type(rules, unit.entity_id).is_some_and(|ty|
                ty.category == ObjectCategory::Vehicle)).count() >= 4
        && (brain.census.units.iter().any(|unit| unit.health_permille < 850
                && unit.traits.combat
                && obs.entity_type(rules, unit.entity_id).is_some_and(|ty|
                    ty.category == ObjectCategory::Vehicle)) || budget.free >= 6_000)
        && brain.census.units.iter().filter(|unit|
            obs.entity_type(rules,unit.entity_id).is_some_and(|ty| ty.gunner)).count() < 2
        && !brain.census.units.iter().any(|unit|
            super::advanced::repair_ifv_loaded(rules, obs, &brain.census, unit.entity_id))
        && let Some(found) = build::class_types(rules, obs, ObjectCategory::Vehicle)
            .filter(|(ty, _)| ty.gunner && !ty.naval)
            .filter(|(carrier, _)| brain.census.units.iter().any(|unit|
                unit.traits.engineer && obs.entity_type(rules, unit.entity_id)
                    .is_some_and(|engineer| super::advanced::gunner_repairs(
                        rules, carrier, engineer.ifv_mode))))
            .min_by_key(|(ty,_)| (obs.cost_of(rules, ty), ty.id.clone()))
    {
        return Some(found);
    }
    if matches!(category, ProductionCategory::Vehicle | ProductionCategory::Aircraft)
        && let Some(decision) = super::siege_counter::purchase(brain,rules,obs,category,budget.free)
    {
        // `None` intentionally holds the line for an unlocked counter it
        // cannot yet pay for; infantry/repair spending retain their own gates.
        return decision;
    }
    if category == ProductionCategory::Vehicle
        && let Some((ty,id)) = scarce_advanced_vehicle(brain,rules,obs,budget.income)
        && obs.cost_of(rules,ty) <= budget.free
    {
        return Some((ty,id));
    }
    if let Some(transport) = expedition_carrier(brain,rules,obs,category,budget.free) {
        return Some(transport);
    }
    // A revealed, defenseless civilian/abandoned yard or factory is a map
    // objective, not merely a by-product of generic infantry recruitment.
    // Ask before a native team suggestion consumes another barracks cycle.
    if category == ProductionCategory::Infantry
        && !brain.director.under_attack_now()
        && !brain.director.recovering_income()
        && let Some(engineer) = neutral_asset_engineer(brain, rules, obs, budget.free)
    {
        return Some(engineer);
    }
    // Reserve a real engineer for a visible, besieged capturable factory or
    // a repairable bridge. Native team preferences and generic infantry
    // composition cannot answer this specialist shortage.
    if category == ProductionCategory::Infantry
        && let Some(engineer) = urgent_engineer(brain, rules, obs, budget.free)
    {
        log::info!("auto play: specialist engineer requested for siege/bridge at {}", obs.tick());
        return Some(engineer);
    }
    if category == ProductionCategory::Infantry
        && let Some(engineer) = maintenance_engineer(brain, rules, obs, budget.free)
    {
        log::info!("auto play: reserve engineer ordered for repair IFV at {}", obs.tick());
        return Some(engineer);
    }
    // A scout-revealed safe derrick is a time-limited opportunity, not a
    // generic unit shortfall. Buy the *first* engineer before the native unit
    // suggestion, but never send a lone engineer into a known enemy base.
    if category == ProductionCategory::Infantry
        && !brain.director.recovering_income()
        && !brain.director.under_attack_now()
        && let Some(found) = early_oil_engineer(brain, rules, obs, budget.free)
    {
        return Some(found);
    }
    // An identified infiltration window should not be starved by the house's
    // native infantry suggestion: it can keep choosing riflemen every frame.
    // This is just the purchase, not permission to run through hostile fire.
    if category == ProductionCategory::Infantry
        && !brain.director.under_attack_now()
        && !brain.director.recovering_income()
        && let Some(found) = early_spy_infiltration(brain, rules, obs, budget.free)
    {
        return Some(found);
    }
    // This is not a fixed "Shock Trooper + IFV" recipe: for each actual
    // buildable infantry type evaluate IFVMode or OpenTransportWeapon on the
    // empty carrier and buy the best cost-adjusted, legal combat seat.
    if category == ProductionCategory::Infantry
        && let Some(found) = combo_passenger_purchase(brain, rules, obs, budget.free)
    {
        return Some(found);
    }
    if category == ProductionCategory::Aircraft
        && let Some(signature) = national_signature(brain, rules, obs,
            ObjectCategory::Aircraft, budget.free)
        && airship_due(brain, rules, obs, signature.0)
    {
        return Some(signature);
    }
    if category == ProductionCategory::Infantry
        && let Some(signature) = national_signature(brain, rules, obs,
            ObjectCategory::Infantry, budget.free)
    {
        return Some(signature);
    }
    if category == ProductionCategory::Vehicle && budget.free >= vehicle_floor
        && brain.census.harvesters_served()
        && let Some(signature) = national_signature(brain, rules, obs,
            ObjectCategory::Vehicle, budget.free)
    {
        return Some(signature);
    }
    // A control weapon has no ordinary DPS score. Give Yuri a small real
    // control corps *before* the native generic infantry/tank suggestion,
    // otherwise this capability may never reach the factory at all.
    if matches!(category, ProductionCategory::Infantry | ProductionCategory::Vehicle)
        && let Some(specialist) = mind_control_purchase(brain, rules, obs,
            if category == ProductionCategory::Vehicle {
                ObjectCategory::Vehicle
            } else { ObjectCategory::Infantry }, budget.free)
    {
        return Some(specialist);
    }
    if let Some(found) = chosen_product(brain, rules, obs, category, budget.free)
        && (category != ProductionCategory::Aircraft
            || airship_due(brain, rules, obs, found.0))
    {
        let traits = census::Traits::of(rules, found.0);
        if !(brain.director.protecting_base() || brain.director.recovering_income())
            || !(traits.engineer || traits.spy)
        {
            return Some(found);
        }
    }

    if category == ProductionCategory::Vehicle {
        // A maker bought while the yard stands is for a second base, not for the
        // one that stands. The war factory spends one cycle on it, and it walks
        // out and deploys where the house has ground of its own
        // (`super::expand`).
        if super::expand::wants_a_maker(brain, rules, obs, budget)
            && let Some(maker) = super::expand::yard_maker(rules, obs)
        {
            return Some(maker);
        }
        // A harvester first while the bays are short of them, but only one the
        // house can actually buy: a harvester that needs a bay the house has
        // not built (retail's `[CMIN] Prerequisite=GAWEAP,PROC`) must not hold
        // the line idle while the army waits for a tank.
        if !brain.census.harvesters_served()
            && let Some(found) = build::harvester(rules, obs)
        {
            return Some(found);
        }
        // The war factory waits for the price of one line vehicle, not a heavy
        // one: below it there is nothing it could start, and above it the
        // barracks has stopped spending the wallet it is waiting for
        // (`fill_lines`).
        if budget.free < vehicle_floor {
            return None;
        }
        // A naval type is a `Vehicle` in the rules, so the war factory has to
        // ask for the land ones: a destroyer queued at the war factory is a
        // request the simulation refuses every frame.
        return fighter(
            brain,
            rules,
            obs,
            budget,
            ObjectCategory::Vehicle,
            Some(false),
            infantry_short,
        );
    }

    // A tool before an army: the buildings are taken with an engineer and a
    // spy, and neither is made by the army's own line — `fighter` ranks by
    // combat weight and asks `is_combat_unit`, which refuses an engineer
    // outright and refuses a spy for carrying no weapon. Correctly: neither is
    // what the fist is made of. So the want is stated here, in numbers.
    if category == ProductionCategory::Infantry
        && !brain.director.under_attack_now()
        && !brain.director.recovering_income()
        && let Some(found) = tool(rules, obs, &brain.census, budget.free, &brain.engineers,
            obs.tick() >= brain.next_spy_purchase,
            !brain.spy_sites.is_empty(), !brain.crate_sightings.is_empty())
    {
        return Some(found);
    }

    if budget.free < INFANTRY_MIN_CREDITS {
        return None;
    }
    match category {
        ProductionCategory::Infantry => fighter(
            brain,
            rules,
            obs,
            budget,
            ObjectCategory::Infantry,
            None,
            infantry_short,
        ),
        ProductionCategory::Aircraft => {
            // The aircraft line uses actual rules prices, not the tank's
            // 2,000-credit threshold. Harriers often cost more than the
            // ordinary 1,000-credit unit band but less than 2,000; the old
            // floor + fighter's generic ceiling made an airfield idle even
            // with enough cash to buy its plane.
            fighter(
                brain,
                rules,
                obs,
                budget,
                ObjectCategory::Aircraft,
                None,
                infantry_short,
            )
        }
        ProductionCategory::Ship => {
            if budget.free < COMBAT_MIN_CREDITS {
                return None;
            }
            // Naval units are `Vehicle` types carrying `Naval=`: the rules'
            // own category, and the one `class_types` indexes.
            fighter(
                brain,
                rules,
                obs,
                budget,
                ObjectCategory::Vehicle,
                Some(true),
                infantry_short,
            )
        }
        // The vehicle line answered above, and the two structure lines are
        // `build`'s.
        ProductionCategory::Vehicle
        | ProductionCategory::Building
        | ProductionCategory::Defense => None,
    }
}

/// Invest in an *already discovered and currently unguarded* income point.
/// Acquiring the unit is not an order to capture: `orders::captures` still
/// checks the entire approach and escort safety before risking its life.
fn early_oil_engineer<'r>(
    brain: &AssistBrain,
    rules: &'r RuleSet,
    obs: &Observation<'_>,
    credits: i32,
) -> Option<(&'r ObjectType, InternedId)> {
    if credits < 1_000 || brain.census.refineries == 0
        || !brain.engineers.may_buy(obs.tick())
        || brain.census.units.iter().any(|unit| unit.traits.engineer)
        || !brain.census.units.iter().any(|unit| unit.traits.scout) {
        return None;
    }
    let safe_oil = obs.capturable_buildings(rules).any(|target| {
        // An enemy engineer taking the derrick does not erase its income or
        // our right to take it back. Require live sight of an enemy-held one;
        // a neutral derrick only needs to be explored previously.
        (!obs.is_hostile(target.entity_id)
            || obs.is_cell_visible(target.cell.0, target.cell.1))
            && obs.is_cell_revealed(target.cell.0, target.cell.1)
            && obs.object_type(rules, target.type_id)
                .is_some_and(|ty| ty.produce_cash_amount > 0)
            && !obs.visible_enemy_entities().any(|enemy| {
                enemy.entity_id != target.entity_id
                    && super::squared_distance(enemy.cell, target.cell) <= 12 * 12
            })
            && !brain.census.enemies.iter().any(|enemy| {
                // The prize itself is an enemy-owned building now, not the
                // enemy's main base guarding it; do not block recapture just
                // because the income building has changed hands.
                enemy.entity_id != target.entity_id
                    && obs.object_type(rules, enemy.type_id)
                        .is_some_and(|ty| ty.category == ObjectCategory::Building)
                    && super::squared_distance(enemy.cell, target.cell) <= 12 * 12
            })
    });
    if !safe_oil { return None; }
    build::class_types(rules, obs, ObjectCategory::Infantry)
        .filter(|(ty, _)| census::Traits::of(rules, ty).engineer)
        .filter(|(ty, _)| obs.cost_of(rules, ty) <= credits)
        .min_by_key(|(ty, id)| (obs.cost_of(rules, ty), *id))
}

/// Buy one spy when scouting has found a *currently visible* infiltratable
/// building. Do not rely on the native infantry suggestion being absent: it
/// often continuously recommends basic infantry. A separate 900-frame clock
/// and a one-spy limit keep this from turning into an infiltration spam loop.
fn early_spy_infiltration<'r>(
    brain: &AssistBrain,
    rules: &'r RuleSet,
    obs: &Observation<'_>,
    credits: i32,
) -> Option<(&'r ObjectType, InternedId)> {
    if credits < 900 || brain.census.refineries == 0
        || brain.census.harvesters == 0
        || obs.tick() < brain.next_spy_purchase
        || brain.census.units.iter().any(|unit| unit.traits.spy)
    {
        return None;
    }
    // An actual visible spyable target is an opportunity by itself. The old
    // combined scout+engineer prerequisite made espionage disappear after a
    // single engineer died, although the spy uses neither unit to infiltrate.
    let target_visible = obs.spyable_buildings(rules)
        .any(|building| obs.is_cell_visible(building.cell.0, building.cell.1));
    if !target_visible && brain.spy_sites.is_empty() && !(credits >= 6_000
        && brain.census.units.iter().any(|unit| unit.traits.scout)
        && brain.census.units.iter().any(|unit| unit.traits.engineer))
    {
        return None;
    }
    build::class_types(rules, obs, ObjectCategory::Infantry)
        .filter(|(ty, _)| census::Traits::of(rules, ty).spy)
        .filter(|(ty, _)| obs.cost_of(rules, ty) <= credits)
        .min_by_key(|(ty, id)| (obs.cost_of(rules, ty), *id))
}

/// Buy a rule-driven country-exclusive unit instead of letting the generic
/// side AI choose the same line tank forever. The roster band is bounded, so
/// the signature unit never replaces all of the ordinary front line. Korean
/// aircraft receive the first opportunity, because an empty airfield has no
/// aircraft pool with which to satisfy a normal ratio.
fn national_signature<'r>(
    brain: &AssistBrain, rules: &'r RuleSet, obs: &Observation<'_>,
    category: ObjectCategory, credits: i32,
) -> Option<(&'r ObjectType, InternedId)> {
    if brain.director.recovering_income() || brain.director.protecting_base()
        || brain.push_gate == "need-siege"
        || brain.census.refineries == 0 || brain.census.harvesters == 0
        || credits < 1_500 || obs.side_index() == Some(2)
    { return None; }
    let country = obs.country()?;
    let pool: usize = brain.census.tiers[slot_category(category).index()].iter().sum();
    let signatures = brain.census.units.iter()
        .filter(|unit| obs.object_type(rules, unit.type_id).is_some_and(|ty|
            ty.category == category && build::country_special(ty, &country)))
        .count();
    if !signature_due(category, pool, signatures) { return None; }
    build::class_types(rules, obs, category)
        .filter(|(ty,_)| category != ObjectCategory::Vehicle || !ty.naval)
        .filter(|(ty,_)| super::orders::is_combat_unit(rules, ty))
        .filter(|(ty,_)| build::country_special(ty, &country))
        .filter(|(ty,_)| super::doctrine::may_produce(brain,rules,obs,ty))
        .filter(|(ty,_)| obs.cost_of(rules, ty) <= credits)
        .min_by_key(|(ty,id)| (
            brain.census.type_count(*id),
            std::cmp::Reverse(obs.cost_of(rules,ty)), *id,
        ))
}

fn signature_due(category: ObjectCategory, pool: usize, signatures: usize) -> bool {
    if category == ObjectCategory::Aircraft {
        signatures.saturating_mul(2) <= pool
    } else {
        pool >= 4 && signatures.saturating_mul(5) < pool
    }
}

/// Mind-control is a strategic capability, not a damage-per-second role.
/// Keep at most roughly one control unit per eight fighters, and let normal
/// production resume while any existing controller can still do its work.
fn mind_control_purchase<'r>(
    brain: &AssistBrain, rules: &'r RuleSet, obs: &Observation<'_>,
    category: ObjectCategory, credits: i32,
) -> Option<(&'r ObjectType, InternedId)> {
    if obs.side_index() != Some(2) || credits < 2_000
        || brain.census.fighters < 6
        || brain.director.recovering_income() || brain.director.protecting_base()
        || brain.census.refineries == 0 || brain.census.harvesters == 0
    { return None; }
    let existing = brain.census.units.iter().filter(|unit| {
        obs.object_type(rules, unit.type_id).is_some_and(|ty|
            crate::rules::capabilities::Capabilities::of(rules, ty)
                .has(crate::rules::capabilities::Tag::MindControl))
    }).count();
    if !control_specialist_due(brain.census.fighters, existing) { return None; }
    build::class_types(rules, obs, category)
        .filter(|(ty,_)| !ty.naval && super::orders::is_combat_unit(rules,ty))
        .filter(|(ty,_)| crate::rules::capabilities::Capabilities::of(rules, ty)
            .has(crate::rules::capabilities::Tag::MindControl))
        .filter(|(ty,_)| super::doctrine::may_produce(brain,rules,obs,ty))
        .filter(|(ty,_)| obs.cost_of(rules, ty) <= credits)
        .min_by_key(|(ty,id)| (brain.census.type_count(*id),
            obs.cost_of(rules,ty), *id))
}

fn control_specialist_due(fighters: usize, controllers: usize) -> bool {
    fighters >= 6 && controllers.saturating_mul(8) < fighters
}

/// The type the house's own choice has this factory make, when the house may
/// still build it and pay for it: `Suggest_New_Object 0x004FBD80`.
///
/// Two guards, both the caller's, because the native reader is unchecked: the
/// type must still be buildable (a choice can outlive its factory or its
/// prerequisite) and affordable. A harvester is skipped too — the harvester
/// count above is the economy's own owner, and the slot can still name one
/// after the count is met.
fn chosen_product<'r>(
    brain: &AssistBrain,
    rules: &'r RuleSet,
    obs: &Observation<'_>,
    category: ProductionCategory,
    credits: i32,
) -> Option<(&'r ObjectType, InternedId)> {
    let object_category = match category {
        ProductionCategory::Infantry => ObjectCategory::Infantry,
        ProductionCategory::Vehicle | ProductionCategory::Ship => ObjectCategory::Vehicle,
        ProductionCategory::Aircraft => ObjectCategory::Aircraft,
        ProductionCategory::Building | ProductionCategory::Defense => return None,
    };
    let (ty, type_id) = obs.chosen_product(rules, object_category)?;
    if ty.resource_gatherer
        || !super::doctrine::may_produce(brain,rules,obs,ty)
        || obs.can_build(rules, ty, true) == crate::sim::production::CanBuild::No
        || credits < obs.cost_of(rules, ty)
    {
        return None;
    }
    Some((ty, type_id))
}

/// Slow, unbound, armoured hover aircraft are strategic siege investments,
/// not airfield filler. A limited ratio to the **ground** army stops a rich
/// house producing nothing but airships while its tanks leave one by one.
/// Tactical identity is derived from rules properties rather than type names.
fn heavy_airship(ty: &ObjectType) -> bool {
    ty.category == ObjectCategory::Aircraft && ty.balloon_hover
        && !ty.airport_bound && ty.strength >= 500
}

fn airship_ratio_allows(ground_vehicles: usize, airships: usize) -> bool {
    ground_vehicles >= 6 && airships < (ground_vehicles / 8).max(1).min(3)
}

fn airship_due(
    brain: &AssistBrain, rules: &RuleSet, obs: &Observation<'_>,
    candidate: &ObjectType,
) -> bool {
    if !heavy_airship(candidate) { return true; }
    if brain.director.recovering_income() || brain.director.protecting_base()
        || brain.census.refineries < 2 || brain.census.harvesters < 2
    { return false; }
    let ground = brain.census.tiers[Slot::Vehicle.index()].iter().sum();
    let airships = brain.census.units.iter().filter(|unit| {
        obs.object_type(rules, unit.type_id).is_some_and(heavy_airship)
    }).count();
    airship_ratio_allows(ground, airships)
}

/// High-value adversaries are different problems, not one army-points number.
/// Use the live rules: an airship needs a projectile with AA capability, a
/// standoff tank needs a real ground weapon reaching it, and an armoured tank
/// needs positive verses against its armour. A Kirov, Prism or Apocalypse in
/// a mod with a new name retains its tactical properties.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ThreatClass { Airborne, Saboteur, Explosive, Standoff, HeavyArmour }

fn threat_class(rules: &RuleSet, ty: &ObjectType) -> Option<ThreatClass> {
    // Most retail bombers including the Kirov are VehicleTypes, not
    // AircraftTypes. Effective flight is a movement rule, not an array slot.
    if (ty.category == ObjectCategory::Aircraft || ty.balloon_hover || ty.jumpjet)
        && ty.strength >= 500 {
        return Some(ThreatClass::Airborne);
    }
    if ty.c4 { return Some(ThreatClass::Saboteur); }
    if (0..ty.weapon_record_count()).any(|slot|
        ty.weapon_at(slot).and_then(|name| rules.weapon(name))
            .is_some_and(|weapon| weapon.suicide))
        || (ty.explodes && ty.death_weapon.is_some()) {
        return Some(ThreatClass::Explosive);
    }
    if ty.category != ObjectCategory::Vehicle { return None; }
    let reach = (0..ty.weapon_record_count()).filter_map(|slot| {
        let weapon = ty.weapon_at(slot).and_then(|name| rules.weapon(name))?;
        (!weapon.never_use).then_some(super::advance::weapon_reach_cells(weapon))
    }).max().unwrap_or(0);
    if reach >= 7 { Some(ThreatClass::Standoff) }
    else if ty.strength >= 600 { Some(ThreatClass::HeavyArmour) }
    else { None }
}

fn counter_quality(
    rules: &RuleSet, candidate: &ObjectType,
    enemy: &ObjectType, kind: ThreatClass,
) -> i32 {
    let hostile_reach = super::advance::weapon_range_cells(rules, enemy, false);
    (0..candidate.weapon_record_count()).filter_map(|slot| {
        let weapon = candidate.weapon_at(slot).and_then(|name| rules.weapon(name))?;
        if weapon.never_use || weapon.damage <= 0 { return None; }
        let projectile = rules.weapon_projectile(weapon)?;
        let valid = match kind {
            ThreatClass::Airborne => projectile.aa,
            ThreatClass::Saboteur | ThreatClass::Explosive
                | ThreatClass::Standoff | ThreatClass::HeavyArmour => projectile.ag,
        };
        if !valid { return None; }
        let head = weapon.warhead.as_deref().and_then(|name| rules.warhead(name))?;
        let mult = f64::from_bits(head.interaction(enemy.armor_id()).multiplier().bits());
        if mult <= 0.0 { return None; }
        let reach = super::advance::weapon_reach_cells(weapon);
        if kind == ThreatClass::Standoff && reach + 2 < hostile_reach { return None; }
        // An explosive truck must be intercepted *before* its detonation
        // radius. A point-blank gun is not an economic counter even when its
        // theoretical DPS is high.
        if kind == ThreatClass::Explosive && reach < 5 { return None; }
        let damage = (f64::from(weapon.damage) * mult).clamp(0.0, 8_000.0) as i32;
        let output = damage.saturating_mul(90) / weapon.rof.max(15);
        Some(output.saturating_add(reach.saturating_mul(4)))
    }).max().unwrap_or(0)
}

fn emergency_counter<'r>(
    brain: &AssistBrain,
    rules: &'r RuleSet,
    obs: &Observation<'_>,
    candidates: &[(&'r ObjectType, InternedId, i32)],
) -> Option<(&'r ObjectType, InternedId)> {
    let threat = brain.census.enemies.iter()
        .filter(|enemy| obs.is_cell_visible(enemy.cell.0, enemy.cell.1))
        .filter(|enemy| {
            brain.census.base_cell.is_some_and(|base| {
                super::advance::cell_distance(enemy.cell,base) <= 44
            }) || brain.push_target.is_some_and(|(_,_,front)| {
                super::advance::cell_distance(enemy.cell, front) <= 20
            })
        })
        .filter_map(|enemy| {
            let ty = obs.object_type(rules, enemy.type_id)?;
            let kind = threat_class(rules,ty)?;
            Some((ty, kind))
        })
        .max_by_key(|(ty,kind)| (
            match kind {
                ThreatClass::Explosive => 5, ThreatClass::Saboteur => 4,
                ThreatClass::Airborne => 3, ThreatClass::Standoff => 2,
                ThreatClass::HeavyArmour => 1,
            },ty.strength))?;
    let (enemy, kind) = threat;
    // If the house already fields two real counters per visible dangerous
    // target, do not build nothing but anti-air trucks all match.
    let protected = brain.census.units.iter()
        .filter(|unit| unit.controllable() && unit.traits.combat)
        .filter(|unit| obs.object_type(rules, unit.type_id)
            .is_some_and(|ty| counter_quality(rules,ty,enemy,kind)>0))
        .count();
    let attackers = brain.census.enemies.iter()
        .filter(|unit| obs.is_cell_visible(unit.cell.0,unit.cell.1))
        .filter(|unit| obs.object_type(rules,unit.type_id)
            .is_some_and(|ty| threat_class(rules,ty)==Some(kind)))
        .count();
    if protected >= attackers.saturating_mul(2).max(2) { return None; }
    candidates.iter().copied().filter_map(|(ty,id,_)| {
        let score = counter_quality(rules,ty,enemy,kind);
        (score>0).then_some((score, ty, id))
    }).max_by_key(|(score,ty,id)| (*score, ty.strength, std::cmp::Reverse(*id)))
        .map(|(_,ty,id)| (ty,id))
}

/// The fighting type of `category` the line should start next.
///
/// The gap closes first: a type is what it brings to a job the army cannot
/// already answer. Only when nothing is short does the price band decide, and
/// then the type the house holds fewest of — so a mixed army forms instead of
/// one type forever.
fn fighter<'r>(
    brain: &mut AssistBrain,
    rules: &'r RuleSet,
    obs: &Observation<'_>,
    budget: &Budget,
    category: ObjectCategory,
    naval: Option<bool>,
    infantry_short: bool,
) -> Option<(&'r ObjectType, InternedId)> {
    let can_field_advanced = matches!(category, ObjectCategory::Vehicle | ObjectCategory::Infantry)
        && brain.census.refineries >= 1
        && brain.census.harvesters >= 1
        && brain.census.fighters >= 10
        && (budget.free >= 6_000
            || (brain.census.refineries >= 2 && budget.free >= 3_000))
        && !brain.director.recovering_income()
        && !brain.director.protecting_base();
    let ceiling = if category == ObjectCategory::Aircraft || can_field_advanced {
        budget.free
    } else {
        unit_ceiling(budget.free)
    };
    let affordable: Vec<(&ObjectType, InternedId, i32)> = build::class_types(rules, obs, category)
        .filter(|(ty, _)| naval.is_none_or(|wanted| ty.naval == wanted))
        .filter(|(ty, _)| super::orders::is_combat_unit(rules, ty))
        .filter(|(ty, _)| super::doctrine::may_produce(brain,rules,obs,ty))
        .filter(|(ty, _)| category != ObjectCategory::Aircraft
            || airship_due(brain, rules, obs, ty))
        .filter_map(|(ty, type_id)| {
            let cost = obs.cost_of(rules, ty);
            (cost <= ceiling).then_some((ty, type_id, cost))
        })
        .collect();
    if affordable.is_empty() {
        return None;
    }

    // Hard threats demand a matching weapon, not simply more army points.
    // This runs before the normal price/role scorer but yields once two real
    // counter-capable units exist per visible high-threat attacker.
    if let Some((ty,id)) = emergency_counter(brain, rules, obs, &affordable) {
        return Some((ty,id));
    }

    // The previous push identified a real, visible battery that outranges
    // everything in the fist. Buy an unlocked standoff vehicle rather than
    // another short-range tank to lose under the guns. A siege asset is only
    // bought when affordable; other categories keep their normal flow.
    if category == ObjectCategory::Vehicle && brain.push_gate == "need-siege" {
        // Use the reach measured at the *current attack target*. A huge gun
        // on the opposite edge of the map must not make every affordable
        // artillery candidate appear inadequate.
        let target_reach = brain.siege_reach.unwrap_or(0);
        let defended = brain.push_target.map(|(_,_,cell)| cell);
        if let Some((ty,id,_)) = affordable.iter().copied()
            .filter(|(ty,_,_)| (0..ty.weapon_record_count()).any(|slot| {
                let Some(weapon) = ty.weapon_at(slot)
                    .and_then(|name| rules.weapon(name)) else { return false; };
                let Some(warhead) = weapon.warhead.as_deref()
                    .and_then(|name| rules.warhead(name)) else { return false; };
                !weapon.never_use
                    && super::advance::weapon_reach_cells(weapon) > target_reach + 1
                    && defended.is_some_and(|center| brain.census.enemies.iter().any(|enemy| {
                        let Some(defense) = obs.object_type(rules, enemy.type_id)
                        else { return false; };
                        defense.is_base_defense
                            && super::advance::cell_distance(center, enemy.cell) <= 14
                            && f64::from_bits(warhead.interaction(defense.armor_id())
                                .multiplier().bits()) > 0.0
                    }))
            }))
            .min_by_key(|(_,id,cost)| (brain.census.type_count(*id), *cost, *id))
        { return Some((ty,id)); }
    }

    // A campaign which has already secured income converts surplus into
    // endgame strength. This reads the live rule-derived role weights and the
    // current legal production choices: a mod's best siege/armour unit may
    // have a completely different type name, weapon and cost band.
    if category == ObjectCategory::Vehicle && brain.campaign.prefer_quality()
        && can_field_advanced
    {
        let pool: usize = brain.census.tiers[slot_category(category).index()].iter().sum();
        let line_price = super::economy::line_vehicle_floor(rules, obs)
            .unwrap_or(COMBAT_MIN_CREDITS).max(1);
        let heavy: Vec<(&ObjectType, InternedId, i32)> = affordable.iter().copied()
            .filter(|(_,_,cost)| *cost >= line_price.saturating_mul(3) / 2)
            .collect();
        let heavy_held: usize = brain.census.by_type.iter().filter_map(|(id,count)| {
            obs.object_type(rules, *id).filter(|ty| {
                ty.category == ObjectCategory::Vehicle
                    && obs.cost_of(rules, ty) >= line_price.saturating_mul(3) / 2
            }).map(|_| *count)
        }).sum();
        if !heavy.is_empty() && pool >= 6 && heavy_held.saturating_mul(5) < pool.saturating_mul(2) {
            let scored: Vec<_> = {
                let roles = brain.roles(rules);
                heavy.iter().copied().map(|(ty,id,cost)| {
                    let weight = roles.weights(&ty.id);
                    // Structure-busting and stand-off fire matter for the
                    // final assault; armoured duels and AA remain valuable.
                    let score = 0.45 * weight.get(Role::AntiStructure)
                        + 0.4 * weight.get(Role::Artillery)
                        + 0.25 * weight.get(Role::AntiVehicle)
                        + 0.10 * weight.get(Role::AntiAir);
                    (ty,id,cost,score)
                }).collect()
            };
            if let Some((ty,id,_,_)) = scored.into_iter()
                .filter(|(_,_,_,score)| *score > 0.0)
                .max_by(|a,b| a.3.total_cmp(&b.3)
                    .then_with(|| brain.census.type_count(b.1).cmp(&brain.census.type_count(a.1)))
                    .then_with(|| b.2.cmp(&a.2)))
            { return Some((ty,id)); }
        }
    }

    // Reserve a bounded share for unlocked advanced weapons. Without this,
    // the shortfall scorer's preference for cheaper answers can buy line
    // tanks forever even after a tech lab unlocks Prism/Mirage/Apocalypse.
    // Keep the quota small: expensive units cannot erase the line army.
    if can_field_advanced {
        let slot = slot_category(category).index();
        let pool: usize = brain.census.tiers[slot].iter().sum();
        let advanced = brain.census.tiers[slot][2];
        if advanced_due(pool, advanced) {
            if let Some((ty, id, _)) = affordable.iter().copied()
                .filter(|(_, _, cost)| price_tier(*cost) == 2)
                .min_by_key(|(_, id, cost)| (brain.census.type_count(*id), -cost, *id))
            {
                return Some((ty, id));
            }
        }
    }

    // The infantry floor outranks the ranking: below the share, the barracks
    // buys the cheapest soldier it can pay for and the line keeps firing.
    if category == ObjectCategory::Infantry && infantry_short {
        return affordable
            .iter()
            .copied()
            .min_by_key(|(_, _, cost)| *cost)
            .map(|(ty, type_id, _)| (ty, type_id));
    }

    let need = brain.census.shortfall();
    let tiers = brain.census.tiers;
    if let Some(need) = need {
        // The table is borrowed for the scoring pass only: the fallback below
        // reads the ledger, and one `&mut brain` cannot be both.
        // How many of each candidate the house already holds, read before the
        // role table is borrowed: `brain.roles` takes `&mut brain`, and the
        // ledger is one of its fields.
        let held: Vec<i32> = affordable
            .iter()
            .map(|(_, type_id, _)| {
                i32::try_from(brain.census.type_count(*type_id)).unwrap_or(i32::MAX)
            })
            .collect();
        let mut best: Option<(f64, i32, i32, &ObjectType, InternedId)> = None;
        // Whether each candidate closes any gap at all: read with the role
        // table, and kept for the cap below, which runs after the borrow.
        let mut closes: Vec<bool> = vec![false; affordable.len()];
        {
            let roles = brain.roles(rules);
            for (index, (ty, type_id, cost)) in affordable.iter().copied().enumerate() {
                let weights = roles.weights(&ty.id);
                // **The biggest gap it closes**, not the sum of all of them: a
                // type that answers four jobs a little is not four units, and
                // summing let one multi-role vehicle outrank every specialist
                // — which is what a play test sees as "still the IFV thing".
                let score = ROLES
                    .iter()
                    .map(|role| weights.get(*role) * f64::from(need[role.index()].min(GAP_CAP)))
                    .fold(0.0f64, f64::max);
                closes[index] = score > 0.0;
                // Between two types that close the same gap, the one the house
                // holds fewest of. One gap, one answer, and the answer with the
                // best score won every cycle for as long as the gap lasted —
                // the war factory made twenty of the same tank while the gap
                // that made it the answer was still there. The count is what
                // the fallback below ranks by; using it here too is what makes
                // the line rotate between the types that answer the job.
                let held = held[index];
                // Within the same answer, the cheaper type: a gap is closed by
                // one unit whether it costs 200 or 900.
                if best.is_none_or(|(held_score, held_held, held_cost, _, _)| {
                    (score, -held, -cost) > (held_score, held_held, held_cost)
                }) {
                    best = Some((score, -held, -cost, ty, type_id));
                }
            }
        }
        // A gap is a reason for the first answer, not for the tenth. Once a
        // type is most of the pool the gap it closes is covered by what the
        // house already has, and every further one is the line making the same
        // unit again — the play test's "it still only makes IFVs" while the
        // operator's real problem is that the pool has one shape. The next
        // type that closes any gap takes the cycle instead; with nothing else
        // scoring, the capped type still answers, because a line that makes
        // nothing is worse than a line that repeats.
        let pool: usize = tiers[slot_category(category).index()].iter().sum();
        if let Some((score, _, _, ty, type_id)) = best
            && score > 0.0
        {
            let cap = pool.max(1) * TYPE_SHARE_PERCENT;
            let over = pool >= TYPE_SHARE_MIN_POOL && brain.census.type_count(type_id) * 100 >= cap;
            if over {
                let alternative = affordable
                    .iter()
                    .copied()
                    .enumerate()
                    .filter(|(index, (_, id, _))| *id != type_id && closes[*index])
                    .filter(|(_, (_, id, _))| brain.census.type_count(*id) * 100 < cap)
                    .min_by_key(|(_, (_, id, cost))| (brain.census.type_count(*id), -cost, *id));
                if let Some((_, (candidate, id, _))) = alternative {
                    return Some((candidate, id));
                }
            }
            return Some((ty, type_id));
        }
    }

    // Nothing is short: the thinnest price band that has anything the house can
    // pay for, and inside it the type the house holds fewest of. The dearest
    // breaks ties, because a band is a price and not a preference.
    //
    // The bands are tried in order rather than picking one and hoping: the
    // cheapest band often holds no fighting type at all (a band under 500 is
    // soldiers and the odd light vehicle), and returning "nothing" there is
    // what kept the war factory idle while the house was rich.
    let present = tiers[slot_category(category).index()];
    let mut bands: Vec<usize> = (0..3).collect();
    bands.sort_by_key(|tier| (present[*tier], *tier));
    for band in bands {
        let pick = affordable
            .iter()
            .copied()
            .filter(|(_, _, cost)| price_tier(*cost) == band)
            .min_by_key(|(_, type_id, cost)| (brain.census.type_count(*type_id), -cost));
        if let Some((ty, type_id, _)) = pick {
            return Some((ty, type_id));
        }
    }
    None
}

/// At most one in five combat units should come from the expensive tier.
/// Test it without any retail unit names so modded rules obey the same quota.
fn advanced_due(pool: usize, advanced: usize) -> bool {
    pool >= 5 && advanced.saturating_mul(5) < pool
}

/// The pool a rules category's fighting units are counted in. Naval units are
/// their own pool, which the rules express as `Naval=yes` on a `Vehicle`; a
/// naval build reaches this function through the `Ship` line instead.
fn slot_category(category: ObjectCategory) -> Slot {
    match category {
        ObjectCategory::Infantry => Slot::Infantry,
        ObjectCategory::Vehicle => Slot::Vehicle,
        ObjectCategory::Aircraft => Slot::Aircraft,
        ObjectCategory::Building => Slot::Building,
    }
}

/// How dear a unit the wallet may reach for: cheap while the base is short,
/// the middle while it is solvent, anything once it is rich — and never more
/// than it holds.
fn unit_ceiling(credits: i32) -> i32 {
    // One band under the rich one, not two. The cheap band is the price of a
    // soldier, and a wallet holding one line tank's price was read as "not
    // enough for a vehicle" — the war factory answered nothing while the
    // credits for exactly the tank it should have made sat in the wallet.
    let band = if credits < RICH_CREDITS {
        census::MID_UNIT_CREDITS
    } else {
        i32::MAX
    };
    band.min(credits)
}

/// Whether a building of the house answers `Factory=AircraftType`: the line
/// exists only when one does, and an aircraft type whose prerequisites are met
/// answers `CanBuild` yes without it — the queue refuses the order every frame
/// it is retried.
fn has_naval_factory(rules: &RuleSet, obs: &Observation<'_>) -> bool {
    obs.own_entities().any(|entity| obs.object_type(rules,entity.type_id)
        .is_some_and(|ty| ty.category == ObjectCategory::Building
            && ty.factory_type() == Some(
                crate::rules::object_type::FactoryType::UnitType)
            && (ty.naval || rules.shipyard_types.iter().any(|name| name == &ty.id))))
}

fn has_aircraft_factory(rules: &RuleSet, obs: &Observation<'_>) -> bool {
    obs.own_entities().any(|entity| {
        obs.object_type(rules, entity.type_id).is_some_and(|ty| {
            ty.factory_type() == Some(crate::rules::object_type::FactoryType::AircraftType)
        })
    })
}

fn queue_unit(
    rules: &RuleSet,
    obs: &Observation<'_>,
    budget: &mut Budget,
    out: &mut Outbox,
    wanted: Option<(&ObjectType, InternedId)>,
) -> bool {
    let Some((ty, type_id)) = wanted else {
        return false;
    };
    let cost = obs.cost_of(rules, ty);
    if !budget.try_commit(cost) {
        return false;
    }
    log::debug!(
        "auto play: unit {} at {} (credits remaining {})",
        ty.id,
        cost,
        budget.free
    );
    out.intent(Intent::QueueProduction { type_id });
    true
}

#[cfg(all(test, tests_sim_p_s))]
mod tests {
    use super::*;

    use crate::sim::world::Simulation;

    /// The retail rival arena, needed to field real object types. The rules
    /// come from the repository's own `ini/` snapshot, so the lines' choices are
    /// regression tested without a retail install.
    fn arena(seed: u64) -> Option<(Simulation, RuleSet)> {
        let rules = crate::rules::retail_ini_fixture::retail_unlayered_rules()?;
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, seed);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        Some((sim, rules))
    }

    fn ledger(rules: &RuleSet, sim: &Simulation, owner: InternedId) -> Census {
        let mut census = Census::default();
        census.refresh(
            &crate::rules::unit_role::RoleTable::build(rules),
            rules,
            &Observation::new(sim, owner),
        );
        census
    }

    /// The production line must really obey the strategic income-recovery
    /// directive, not only show it in the log. A cheap soldier may not take
    /// the credits reserved for the house's *last* miner.
    #[test]
    fn the_last_miner_is_queued_before_any_new_soldier() {
        let Some((mut sim, rules)) = arena(0xA17) else {
            return;
        };
        let owner = sim.interner.intern("Americans");
        for (name, cell) in [
            ("GACNST", (8_u16, 8_u16)),
            ("GAPOWR", (12, 8)),
            ("GAREFN", (16, 8)),
            ("GAWEAP", (8, 16)),
            ("GAPILE", (16, 16)),
        ] {
            sim.spawn_object(name, "Americans", cell.0, cell.1, 255, &rules)
                .unwrap_or_else(|| panic!("spawn {name}"));
        }
        sim.houses.get_mut(&owner).unwrap().economy_mut()
            .set_credits_for_test(3_000);
        let mut brain = AssistBrain::new(0xA17);
        brain.census = ledger(&rules, &sim, owner);
        assert_eq!(brain.census.harvesters, 0);
        let obs = Observation::new(&sim, owner);
        brain.director.observe(obs.tick(), super::super::strategy::Signals {
            yards: 1, refineries: 1, harvesters: 0, ..Default::default()
        });
        let budget = Budget { free: 3_000, committed: 0, income: None };
        let mut out = Outbox::default();
        fill_lines(&mut brain, &rules, &obs, &budget, &mut out);
        let first = out.intents.iter().find_map(|intent| match intent {
            Intent::QueueProduction { type_id } => Some(sim.interner().resolve(*type_id)),
            _ => None,
        });
        assert_eq!(first, Some("CMIN"),
            "the war factory buys income before the infantry line spends it: {:?}", out.intents);
    }

    #[test]
    fn the_last_miner_saves_money_instead_of_spending_on_infantry() {
        let Some((mut sim, rules)) = arena(0xA17) else {
            return;
        };
        let owner = sim.interner.intern("Americans");
        for (name, cell) in [
            ("GACNST", (8_u16, 8_u16)),
            ("GAPOWR", (12, 8)),
            ("GAREFN", (16, 8)),
            ("GAWEAP", (8, 16)),
            ("GAPILE", (16, 16)),
        ] {
            sim.spawn_object(name, "Americans", cell.0, cell.1, 255, &rules)
                .unwrap_or_else(|| panic!("spawn {name}"));
        }
        let mut brain = AssistBrain::new(0xA17);
        brain.census = ledger(&rules, &sim, owner);
        let obs = Observation::new(&sim, owner);
        brain.director.observe(obs.tick(), super::super::strategy::Signals {
            yards: 1, refineries: 1, harvesters: 0, ..Default::default()
        });
        let budget = Budget { free: 250, committed: 0, income: None };
        let mut out = Outbox::default();
        fill_lines(&mut brain, &rules, &obs, &budget, &mut out);
        assert!(out.intents.iter().all(|intent| !matches!(intent, Intent::QueueProduction { .. })),
            "when the last miner cannot be bought, retain the wallet instead: {:?}", out.intents);
    }

    /// The barracks is a line that fires. Before the floor, an army with no
    /// infantry counted as short of nothing — the vehicle ranking answered
    /// every job — and the barracks stood idle for the whole match.
    #[test]
    fn a_barracks_below_the_infantry_share_makes_soldiers() {
        let Some((mut sim, rules)) = arena(0xA17) else {
            return;
        };
        let owner = sim.interner.intern("Americans");
        sim.spawn_object("GACNST", "Americans", 16, 16, 255, &rules)
            .expect("a yard");
        sim.spawn_object("GAPOWR", "Americans", 20, 16, 255, &rules)
            .expect("a power plant");
        sim.spawn_object("GAPILE", "Americans", 24, 16, 255, &rules)
            .expect("a barracks");
        sim.houses
            .get_mut(&owner)
            .expect("the house")
            .economy_mut()
            .set_credits_for_test(3_000);

        let mut brain = AssistBrain::new(0xA17);
        brain.census = ledger(&rules, &sim, owner);
        let obs = Observation::new(&sim, owner);
        let mut out = Outbox::default();
        let budget = Budget {
            committed: 0,
            free: obs.credits(),
            income: None,
        };
        fill_lines(&mut brain, &rules, &obs, &budget, &mut out);

        let queued: Vec<&str> = out
            .intents
            .iter()
            .filter_map(|intent| match intent {
                Intent::QueueProduction { type_id } => Some(sim.interner().resolve(*type_id)),
                _ => None,
            })
            .collect();
        assert_eq!(queued.len(), 1, "the idle barracks starts one build");
        let ty = rules.object(queued[0]).expect("a rules type");
        assert_eq!(
            ty.category,
            ObjectCategory::Infantry,
            "the barracks makes a soldier, not nothing: {}",
            queued[0]
        );
    }

    /// A house with no factory of a category has nothing to fill: the type
    /// filter is `CanBuild`, and a type with no factory of its category in the
    /// house's hands answers No. Without that, every frame would queue a
    /// request the simulation refuses.
    #[test]
    fn a_house_with_no_factory_queues_nothing() {
        let Some((mut sim, rules)) = arena(0xA17) else {
            return;
        };
        let owner = sim.interner.intern("Americans");
        sim.spawn_object("GACNST", "Americans", 16, 16, 255, &rules)
            .expect("a yard");
        sim.houses
            .get_mut(&owner)
            .expect("the house")
            .economy_mut()
            .set_credits_for_test(5_000);

        let mut brain = AssistBrain::new(0xA17);
        brain.census = ledger(&rules, &sim, owner);
        let obs = Observation::new(&sim, owner);
        let mut out = Outbox::default();
        let budget = Budget {
            committed: 0,
            free: obs.credits(),
            income: None,
        };
        fill_lines(&mut brain, &rules, &obs, &budget, &mut out);

        assert!(
            out.intents.is_empty(),
            "no barracks, no war factory, nothing to start: {:?}",
            out.intents
        );
    }

    /// A rich house with an idle war factory starts a vehicle. The cheapest
    /// price band holds no fighting vehicle at all, so picking the thinnest band
    /// and giving up when it is empty is how the factory went idle for a whole
    /// match while the wallet filled up.
    #[test]
    fn a_rich_house_with_an_idle_war_factory_starts_a_vehicle() {
        let Some((mut sim, rules)) = arena(0xA17) else {
            return;
        };
        let owner = sim.interner.intern("Americans");
        sim.spawn_object("GACNST", "Americans", 16, 16, 255, &rules)
            .expect("a yard");
        sim.spawn_object("GAPOWR", "Americans", 20, 16, 255, &rules)
            .expect("a power plant");
        sim.spawn_object("GAWEAP", "Americans", 24, 16, 255, &rules)
            .expect("a war factory");
        // A bay and the two miners it keeps busy, so the harvester arm of the
        // vehicle line is satisfied and the fighting rank is what is asked.
        sim.spawn_object("GAREFN", "Americans", 16, 24, 255, &rules)
            .expect("a refinery");
        for cell in [(24u16, 24u16), (26, 24)] {
            sim.spawn_object("CMIN", "Americans", cell.0, cell.1, 255, &rules)
                .expect("a Chrono Miner");
        }
        sim.houses
            .get_mut(&owner)
            .expect("the house")
            .economy_mut()
            .set_credits_for_test(4_000);

        let mut brain = AssistBrain::new(0xA17);
        brain.census = ledger(&rules, &sim, owner);
        let obs = Observation::new(&sim, owner);
        let mut out = Outbox::default();
        let budget = Budget {
            committed: 0,
            free: obs.credits(),
            income: None,
        };
        fill_lines(&mut brain, &rules, &obs, &budget, &mut out);

        let queued: Vec<&str> = out
            .intents
            .iter()
            .filter_map(|intent| match intent {
                Intent::QueueProduction { type_id } => Some(sim.interner().resolve(*type_id)),
                _ => None,
            })
            .collect();
        assert_eq!(queued.len(), 1, "one line, one build: {:?}", out.intents);
        let ty = rules.object(queued[0]).expect("a rules type");
        assert!(
            ty.category == ObjectCategory::Vehicle && !ty.naval,
            "the war factory makes a land vehicle: {}",
            queued[0]
        );
        assert!(
            !ty.resource_gatherer,
            "the miners are already at the bays: {}",
            queued[0]
        );
    }

    /// A house that already owns a few of the dearest cheap vehicle does not buy
    /// another: with nothing short, the pick is the type it holds fewest of. One
    /// type forever — the "still the IFV thing" in a play test — is what the old
    /// ranking did, because the dearest type inside the thinnest band always won
    /// the tie against itself.
    #[test]
    fn the_vehicle_line_does_not_buy_one_type_forever() {
        let Some((mut sim, rules)) = arena(0xA17) else {
            return;
        };
        let owner = sim.interner.intern("Americans");
        for (name, cell) in [
            ("GACNST", (8u16, 8u16)),
            ("GAPOWR", (12, 8)),
            ("GAREFN", (16, 8)),
            ("GAWEAP", (8, 16)),
        ] {
            sim.spawn_object(name, "Americans", cell.0, cell.1, 255, &rules)
                .unwrap_or_else(|| panic!("{name}"));
        }
        for cell in [(16u16, 16u16), (18, 16)] {
            sim.spawn_object("CMIN", "Americans", cell.0, cell.1, 255, &rules)
                .expect("a Chrono Miner");
        }
        for cell in [(20u16, 16u16), (22, 16), (24, 16)] {
            sim.spawn_object("MTNK", "Americans", cell.0, cell.1, 255, &rules)
                .expect("a Grizzly");
        }
        sim.houses
            .get_mut(&owner)
            .expect("the house")
            .economy_mut()
            .set_credits_for_test(6_000);

        let mut brain = AssistBrain::new(0xA17);
        brain.census = ledger(&rules, &sim, owner);
        let obs = Observation::new(&sim, owner);
        let mut out = Outbox::default();
        let budget = Budget {
            committed: 0,
            free: obs.credits(),
            income: None,
        };
        fill_lines(&mut brain, &rules, &obs, &budget, &mut out);

        let queued: Vec<&str> = out
            .intents
            .iter()
            .filter_map(|intent| match intent {
                Intent::QueueProduction { type_id } => Some(sim.interner().resolve(*type_id)),
                _ => None,
            })
            .collect();
        assert_eq!(queued.len(), 1, "{:?}", out.intents);
        assert_ne!(
            queued[0], "MTNK",
            "three of them already stand, so the line buys something else"
        );
    }

    /// The aircraft line is a line like any other once its plant stands. The
    /// plant is the rules' own `Factory=AircraftType` building —
    /// `build::line_factory`, not a name list — and for the Americans that is
    /// `AMRADR` (`RequiredHouses=Americans`), while `GAAIRC` is refused by
    /// `ForbiddenHouses=Americans` (ini/rulesmd.ini:11822).
    #[test]
    fn an_aircraft_line_makes_an_aircraft_once_the_hangar_stands() {
        let Some((mut sim, rules)) = arena(0xA17) else {
            return;
        };
        let owner = sim.interner.intern("Americans");
        for (name, cell) in [
            ("GACNST", (8u16, 8u16)),
            ("GAPOWR", (12, 8)),
            ("GAREFN", (16, 8)),
            ("GAWEAP", (8, 16)),
            ("AMRADR", (14, 16)),
        ] {
            sim.spawn_object(name, "Americans", cell.0, cell.1, 255, &rules)
                .unwrap_or_else(|| panic!("{name}"));
        }
        sim.houses
            .get_mut(&owner)
            .expect("the house")
            .economy_mut()
            .set_credits_for_test(6_000);

        let mut brain = AssistBrain::new(0xA17);
        brain.census = ledger(&rules, &sim, owner);
        let obs = Observation::new(&sim, owner);
        let mut out = Outbox::default();
        let budget = Budget {
            committed: 0,
            free: obs.credits(),
            income: None,
        };
        fill_lines(&mut brain, &rules, &obs, &budget, &mut out);

        let aircraft: Vec<&str> = out
            .intents
            .iter()
            .filter_map(|intent| match intent {
                Intent::QueueProduction { type_id } => {
                    let ty = obs.object_type(&rules, *type_id)?;
                    (ty.category == ObjectCategory::Aircraft)
                        .then(|| sim.interner().resolve(*type_id))
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            aircraft.len(),
            1,
            "an idle air force command HQ starts an aircraft: {:?}",
            out.intents
        );
    }

    /// An army that already holds its share of infantry stops buying it, so
    /// the wallet can reach the price of a tank. A soldier is a tenth of a
    /// tank, so a barracks left to itself spends every credit the house earns
    /// on infantry and the war factory never gets to buy anything — the "only
    /// ever infantry" army.
    #[test]
    fn an_army_past_its_infantry_share_buys_the_vehicle_instead() {
        let Some((mut sim, rules)) = arena(0xA17) else {
            return;
        };
        let owner = sim.interner.intern("Americans");
        for (name, cell) in [
            ("GACNST", (16u16, 16u16)),
            ("GAPOWR", (12, 16)),
            ("GAPILE", (20, 16)),
            ("GAWEAP", (24, 16)),
            // The bays have their harvesters, so the vehicle line is free to
            // spend on the army rather than on income.
            ("CMIN", (28, 16)),
            ("CMIN", (30, 16)),
        ] {
            sim.spawn_object(name, "Americans", cell.0, cell.1, 255, &rules)
                .unwrap_or_else(|| panic!("{name}"));
        }
        // Six soldiers and nothing else: past the ceiling, not past the floor.
        for index in 0..6u16 {
            sim.spawn_object("E1", "Americans", 20 + index, 24, 255, &rules)
                .expect("a soldier");
        }
        sim.houses
            .get_mut(&owner)
            .expect("the house")
            .economy_mut()
            .set_credits_for_test(3_000);

        let mut brain = AssistBrain::new(0xA17);
        brain.census = ledger(&rules, &sim, owner);
        let obs = Observation::new(&sim, owner);
        let mut out = Outbox::default();
        let budget = Budget {
            committed: 0,
            free: obs.credits(),
            income: None,
        };
        fill_lines(&mut brain, &rules, &obs, &budget, &mut out);

        let queued: Vec<&str> = out
            .intents
            .iter()
            .filter_map(|intent| match intent {
                Intent::QueueProduction { type_id } => Some(sim.interner().resolve(*type_id)),
                _ => None,
            })
            .collect();
        for name in &queued {
            let ty = rules.object(name).expect("a rules type");
            assert_ne!(
                ty.category,
                ObjectCategory::Infantry,
                "the barracks stops past the ceiling: {name}"
            );
        }
        assert!(
            queued
                .iter()
                .any(|name| rules.object(name).expect("a rules type").category
                    == ObjectCategory::Vehicle),
            "the wallet reaches the war factory: {queued:?}"
        );
    }

    /// The same army with a wallet the war factory cannot reach keeps the
    /// barracks turning.
    ///
    /// The ceiling is there to save the wallet for a tank. Saving it for a
    /// tank the line cannot pay for is a base producing nothing at all: the
    /// barracks waiting out its ceiling and the war factory waiting out the
    /// vehicle's price, both idle. The ceiling only bites at the price of one
    /// line vehicle, and under it the barracks keeps its own line running.
    #[test]
    fn a_barracks_holds_while_the_wallet_is_below_the_war_factory() {
        let Some((mut sim, rules)) = arena(0xA17) else {
            return;
        };
        let owner = sim.interner.intern("Americans");
        for (name, cell) in [
            ("GACNST", (16u16, 16u16)),
            ("GAPOWR", (12, 16)),
            ("GAPILE", (20, 16)),
            ("GAWEAP", (24, 16)),
            ("CMIN", (28, 16)),
            ("CMIN", (30, 16)),
        ] {
            sim.spawn_object(name, "Americans", cell.0, cell.1, 255, &rules)
                .unwrap_or_else(|| panic!("{name}"));
        }
        for index in 0..6u16 {
            sim.spawn_object("E1", "Americans", 20 + index, 24, 255, &rules)
                .expect("a soldier");
        }
        let obs = Observation::new(&sim, owner);
        let floor = crate::sim::strategy::assist::economy::line_vehicle_floor(&rules, &obs).expect("the side has a line vehicle");
        let under = floor.saturating_sub(100).max(INFANTRY_MIN_CREDITS);
        sim.houses
            .get_mut(&owner)
            .expect("the house")
            .economy_mut()
            .set_credits_for_test(under);

        let mut brain = AssistBrain::new(0xA17);
        brain.census = ledger(&rules, &sim, owner);
        let obs = Observation::new(&sim, owner);
        let mut out = Outbox::default();
        let budget = Budget {
            committed: 0,
            free: obs.credits(),
            income: None,
        };
        assert!(
            budget.free < floor,
            "the war factory cannot buy: {budget:?} against {floor}"
        );
        fill_lines(&mut brain, &rules, &obs, &budget, &mut out);

        let queued: Vec<&str> = out
            .intents
            .iter()
            .filter_map(|intent| match intent {
                Intent::QueueProduction { type_id } => Some(sim.interner().resolve(*type_id)),
                _ => None,
            })
            .collect();
        assert!(
            !queued
                .iter()
                .any(|name| rules.object(name).expect("a rules type").category
                    == ObjectCategory::Infantry),
            "a soldier here would put the saving back: {queued:?}"
        );
    }

    /// A house with no dog makes one, before it makes anything else.
    ///
    /// The dog is the cheapest pair of eyes the house has and the only early
    /// answer to an enemy engineer walking into the base: a house that opens
    /// with its own engineer instead loses the capture race without noticing it
    /// was in one, and the engineer it paid for walks into a building the other
    /// side already took.
    #[test]
    fn a_barracks_opens_with_the_dog() {
        let Some((mut sim, rules)) = arena(0xA17) else {
            return;
        };
        let owner = sim.interner.intern("Americans");
        for (name, cell) in [
            ("GACNST", (16u16, 16u16)),
            ("GAPOWR", (12, 16)),
            ("GAPILE", (20, 16)),
        ] {
            sim.spawn_object(name, "Americans", cell.0, cell.1, 255, &rules)
                .unwrap_or_else(|| panic!("{name}"));
        }
        sim.houses
            .get_mut(&owner)
            .expect("the house")
            .economy_mut()
            .set_credits_for_test(3_000);

        let mut brain = AssistBrain::new(0xA17);
        brain.census = ledger(&rules, &sim, owner);
        let obs = Observation::new(&sim, owner);
        let budget = Budget {
            committed: 0,
            free: obs.credits(),
            income: None,
        };
        let mut out = Outbox::default();
        fill_lines(&mut brain, &rules, &obs, &budget, &mut out);

        let made: Vec<&str> = out
            .intents
            .iter()
            .filter_map(|intent| match intent {
                Intent::QueueProduction { type_id } => Some(sim.interner().resolve(*type_id)),
                _ => None,
            })
            .collect();
        assert!(
            made.iter().any(|name| rules
                .object(name)
                .is_some_and(|ty| ty.not_human && ty.natural)),
            "the opening infantry is the dog: {made:?}"
        );
    }

    /// A spy is not an engineer.
    ///
    /// `Infiltrate=` is forced on `Engineer=`, `C4=` and `Agent=` by retail's
    /// own ReadINI postpass, so counting the house's spies with the
    /// infiltrator tag counted its engineers as spies: with both engineers and
    /// both dogs the house already read as holding a spy, the infantry line's
    /// tool picker answered nothing at all, and not one spy was ever built.
    #[test]
    fn a_house_with_its_engineers_and_dogs_still_builds_a_spy() {
        let Some((mut sim, rules)) = arena(0xB17) else {
            return;
        };
        let owner = sim.interner.intern("Americans");
        for (name, cell) in [
            ("GACNST", (16u16, 16u16)),
            ("GAPOWR", (20, 16)),
            ("GAPILE", (24, 16)),
            ("GATECH", (28, 16)),
        ] {
            sim.spawn_object(name, "Americans", cell.0, cell.1, 255, &rules)
                .unwrap_or_else(|| panic!("{name}"));
        }
        for (name, cell) in [
            ("ENGINEER", (10u16, 24u16)),
            ("ENGINEER", (11, 24)),
            ("ADOG", (12, 24)),
            ("ADOG", (13, 24)),
        ] {
            sim.spawn_object(name, "Americans", cell.0, cell.1, 255, &rules)
                .unwrap_or_else(|| panic!("{name}"));
        }

        let census = ledger(&rules, &sim, owner);
        assert_eq!(
            census
                .units
                .iter()
                .filter(|unit| unit.traits.engineer)
                .count(),
            ENGINEER_WANT,
            "both engineers"
        );
        assert_eq!(
            census.units.iter().filter(|unit| unit.traits.scout).count(),
            DOG_WANT,
            "both dogs"
        );
        assert_eq!(
            census.units.iter().filter(|unit| unit.traits.spy).count(),
            0,
            "no spy yet"
        );

        let obs = Observation::new(&sim, owner);
        let (ty, _) = tool(&rules, &obs, &census, 100_000, &EngineerLedger::default(), true,
            false, false)
            .expect("the infantry line still has a tool to make");
        assert!(ty.agent, "the tool is a spy, not {}", ty.id);
    }

    /// A wallet holding one line tank's price starts the war factory.
    ///
    /// Both numbers used to be the heavy unit's: the ceiling answered "cheap
    /// only" under 1,200 and the line waited for 2,000, so the wallet the tank
    /// needed was spent on soldiers a barracks-length earlier and never got
    /// there. The measured match is an army of infantry and dogs, no tank at
    /// all, and a bank that grew while nothing bought vehicles with it.
    #[test]
    fn a_wallet_holding_one_tank_starts_the_war_factory() {
        let Some((mut sim, rules)) = arena(0xA17) else {
            return;
        };
        let owner = sim.interner.intern("Americans");
        for (name, cell) in [
            ("GACNST", (16u16, 16u16)),
            ("GAPILE", (20, 16)),
            ("GAWEAP", (24, 16)),
        ] {
            sim.spawn_object(name, "Americans", cell.0, cell.1, 255, &rules)
                .unwrap_or_else(|| panic!("{name}"));
        }
        sim.houses
            .get_mut(&owner)
            .expect("the house")
            .economy_mut()
            .set_credits_for_test(800);

        let mut brain = AssistBrain::new(0xA17);
        brain.census = ledger(&rules, &sim, owner);
        // The trucks are not this test's question, and a house short of them
        // is asked for another one before anything else.
        brain.census.harvesters = 2;
        brain.census.refineries = 1;
        let obs = Observation::new(&sim, owner);
        let mut out = Outbox::default();
        let budget = Budget {
            committed: 0,
            free: obs.credits(),
            income: None,
        };
        assert_eq!(budget.free, 800, "one line tank's price, not a heavy one's");
        fill_lines(&mut brain, &rules, &obs, &budget, &mut out);

        let queued: Vec<&str> = out
            .intents
            .iter()
            .filter_map(|intent| match intent {
                Intent::QueueProduction { type_id } => Some(sim.interner().resolve(*type_id)),
                _ => None,
            })
            .collect();
        assert!(
            queued.iter().any(|name| rules
                .object(name)
                .is_some_and(|ty| ty.category == ObjectCategory::Vehicle)),
            "the war factory has something it can pay for: {queued:?}"
        );
    }

    /// The infantry ceiling does not depend on the wallet already reaching a
    /// tank.
    ///
    /// A soldier costs a tenth of a tank, so a barracks willing to spend 200
    /// while the tank costs 700 keeps the wallet under 700 forever. The ceiling
    /// exists to make the wallet reach the vehicle, and gating it on the wallet
    /// already holding a vehicle's price means it never fires: measured on
    /// Arena.mmx seed 0x5EED at 24000 ticks, `roster=[E1:48, MGTK:8,
    /// CAOILD:6]` — 48 riflemen, no tanks, with a war factory standing.
    ///
    /// This pins the predicate itself rather than a budget, because the bug was
    /// the conjunction, not the numbers.
    #[test]
    fn the_infantry_ceiling_does_not_wait_for_a_tank_the_wallet_can_never_reach() {
        let mut census = Census::default();
        // Sixty fighters, fifty of them infantry: past the 55% ceiling.
        census.fighters = 60;
        census.tiers[Slot::Infantry.index()] = [50, 0, 0];
        assert!(
            infantry_above_ceiling(&census),
            "50 of 60 is over the ceiling whatever the wallet holds"
        );
        // The floor is the other side of the same reading, and it must not
        // also be permanently true: an army past the ceiling is not short.
        assert!(!infantry_below_floor(&census));
    }

    /// V11: a stable tier quota should periodically pay for unlocked heavy
    /// combat types, but never build *only* the expensive tier.
    #[test]
    fn unlocked_advanced_units_receive_a_bounded_share() {
        assert!(!advanced_due(0, 0));
        assert!(!advanced_due(4, 0));
        assert!(advanced_due(5, 0));
        assert!(!advanced_due(5, 1));
        assert!(advanced_due(10, 1));
        assert!(!advanced_due(10, 2));
        assert!(advanced_due(16, 3));
    }

    /// A failed engineer rush cannot keep a separate spy expedition idle.
    #[test]
    fn spy_purchase_cooldown_is_not_engineer_failure_debt() {
        let mut brain = AssistBrain::new(42);
        for _ in 0..ENGINEER_WASTE_PATIENCE + 1 {
            brain.engineers.note_bought(100);
        }
        brain.engineers.observe_outcomes(100, 0);
        assert!(!brain.engineers.may_buy(100));
        assert_eq!(brain.next_spy_purchase, 0);
        brain.next_spy_purchase = 100 + 900;
        assert!(!brain.engineers.may_buy(100));
        assert!(1_000 >= brain.next_spy_purchase);
    }

    /// The engineer line stops buying when its errands are not landing.
    ///
    /// An engineer is consumed by success and by failure alike, so the live
    /// count drops to zero after every capture and asks for a replacement the
    /// match has already paid for. The ledger is the ratio that separates the
    /// two, and this pins both halves: a house that keeps taking buildings
    /// keeps buying, and one that spends four engineers per building stops.
    #[test]
    fn the_engineer_ledger_stops_a_line_whose_errands_are_not_landing() {
        let mut ledger = EngineerLedger::default();
        // Nothing bought: the first engineer is the one that opens the line.
        assert!(ledger.may_buy(0));

        // Two taken, then two bought: a good rate, so it keeps buying. The
        // order matters and is the real one — a capture lands before the house
        // knows it may replace the engineer it spent.
        ledger.note_captured();
        ledger.note_captured();
        ledger.note_bought(0);
        ledger.note_bought(0);
        assert!(
            ledger.may_buy(0),
            "two engineers for two buildings is the line working"
        );

        // Eight more bought and none taken: ten bought against two taken
        // passes four per building, so it goes quiet. The boundary is the
        // patience itself, so it is pinned rather than guessed at.
        let mut sunk = ledger;
        for _ in 0..(ENGINEER_WASTE_PATIENCE as usize * 2) {
            sunk.note_bought(0);
        }
        assert_eq!(sunk.bought, 2 + ENGINEER_WASTE_PATIENCE * 2);
        assert_eq!(sunk.captured, 2);
        sunk.observe_outcomes(0, 0);
        assert!(
            !sunk.may_buy(0),
            "ten bought for two taken is 狂造工程师拖垮经济"
        );
        // And quiet is a window, not a death sentence: the front moves.
        assert!(sunk.may_buy(ENGINEER_QUIET_FRAMES));
    }

    #[test]
    fn engineers_still_walking_to_capturable_buildings_are_not_failed_purchases() {
        let mut ledger = EngineerLedger::default();
        for _ in 0..5 { ledger.note_bought(10); }
        ledger.observe_outcomes(10, 5);
        assert!(ledger.may_buy(10));
        ledger.observe_outcomes(20, 1);
        assert!(!ledger.may_buy(20));
        assert!(ledger.may_buy(20 + ENGINEER_QUIET_FRAMES));
    }

    /// A rich side with eight riflemen and a balanced army must save its
    /// barracks cycle for high-value technology, not wait until 50 infantry
    /// have accumulated to enforce the original broad percentage ceiling.
    #[test]
    fn rich_mixed_armies_stop_mass_rifleman_recruitment() {
        assert!(!rich_infantry_saturated(7, 10));
        assert!(rich_infantry_saturated(8, 20));
        assert!(!rich_infantry_saturated(8, 30));
        assert!(rich_infantry_saturated(50, 60));
    }

    /// With fifty infantry and only ten tanks, wealth must fund armoured
    /// capacity instead of allowing even more cheap riflemen. Earlier policy
    /// ignored its ratio ceiling at 100K; that drowned the armoured line.
    #[test]
    fn a_rich_wallet_holds_excess_infantry_until_armour_catches_up() {
        let Some((mut sim, rules)) = arena(0xA17) else {
            return;
        };
        let owner = sim.interner.intern("Americans");
        for (name, cell) in [
            ("GACNST", (16u16, 16u16)),
            ("GAPOWR", (12, 16)),
            ("GAPILE", (20, 16)),
            ("GAWEAP", (24, 16)),
        ] {
            sim.spawn_object(name, "Americans", cell.0, cell.1, 255, &rules)
                .unwrap_or_else(|| panic!("{name}"));
        }
        // Fifty of sixty fighters are infantry: past the 55% ceiling. Built as
        // a ledger rather than as 50 objects: what the ceiling reads is
        // `census.tiers`, and spawning that many soldiers is a fixture the map
        // does not have room for.
        let mut brain = AssistBrain::new(0xA17);
        brain.census = ledger(&rules, &sim, owner);
        brain.census.fighters = 60;
        brain.census.tiers[Slot::Infantry.index()] = [50, 0, 0];
        brain.census.tiers[Slot::Vehicle.index()] = [10, 0, 0];
        assert!(infantry_above_ceiling(&brain.census), "past the ceiling");

        let obs = Observation::new(&sim, owner);
        let floor = crate::sim::strategy::assist::economy::line_vehicle_floor(&rules, &obs).expect("the side has a line vehicle");
        let queued = |sim: &Simulation, out: &Outbox| -> Vec<String> {
            out.intents
                .iter()
                .filter_map(|intent| match intent {
                    Intent::QueueProduction { type_id } => {
                        Some(sim.interner().resolve(*type_id).to_string())
                    }
                    _ => None,
                })
                .collect()
        };

        // Ordinariness: a wallet that cannot buy several vehicles still holds.
        let ordinary = Budget {
            committed: 0,
            free: floor,
            income: None,
        };
        let mut out = Outbox::default();
        fill_lines(&mut brain, &rules, &obs, &ordinary, &mut out);
        let ordinary_queued = queued(&sim, &out);
        assert!(
            ordinary_queued.iter().all(
                |name| rules.object(name).expect("a type").category != ObjectCategory::Infantry
            ),
            "a wallet of one vehicle's price still saves: {ordinary_queued:?}"
        );

        // Riches: enough to buy many tanks, but not permission to flood
        // a force that is already 83% infantry.
        let mut brain = AssistBrain::new(0xA17);
        brain.census = ledger(&rules, &sim, owner);
        brain.census.fighters = 60;
        brain.census.tiers[Slot::Infantry.index()] = [50, 0, 0];
        brain.census.tiers[Slot::Vehicle.index()] = [10, 0, 0];
        let mut out = Outbox::default();
        let rich = Budget {
            committed: 0,
            free: floor.saturating_mul(RICH_VEHICLES),
            income: None,
        };
        fill_lines(&mut brain, &rules, &obs, &rich, &mut out);
        let rich_queued = queued(&sim, &out);
        assert!(
            rich_queued.iter().all(
                |name| rules.object(name).expect("a type").category != ObjectCategory::Infantry
                    || rules.object(name).is_some_and(|ty| {
                        super::census::Traits::of(&rules, ty).engineer
                            || super::census::Traits::of(&rules, ty).spy
                    })
            ),
            "rich money should not buy more riflemen before tanks: {rich_queued:?}"
        );
    }
}

#[cfg(all(test, tests_sim_p_s))]
mod v17_faction_tests {
    use super::*;

    #[test]
    fn airships_require_a_ground_screen_and_remain_a_minority() {
        assert!(!airship_ratio_allows(0, 0));
        assert!(!airship_ratio_allows(5, 0));
        assert!(airship_ratio_allows(6, 0));
        assert!(!airship_ratio_allows(6, 1));
        assert!(airship_ratio_allows(16, 1));
        assert!(!airship_ratio_allows(16, 2));
        assert!(airship_ratio_allows(24, 2));
        assert!(!airship_ratio_allows(80, 3));
    }

    #[test]
    fn the_retail_heavy_airship_is_derived_from_type_data() {
        let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules() else {
            return;
        };
        if let Some(zeppelin) = rules.object("ZEP") {
            assert!(heavy_airship(zeppelin));
        }
        if let Some(jet) = rules.object("ORCA") {
            assert!(!heavy_airship(jet));
        }
    }

    #[test]
    fn each_side_keeps_a_different_infantry_budget() {
        assert_eq!(infantry_targets(Some(0)), (40,55));
        assert_eq!(infantry_targets(Some(1)), (25,42));
        assert_eq!(infantry_targets(Some(2)), (35,52));
        assert!(control_specialist_due(8, 0));
        assert!(!control_specialist_due(8, 1));
        assert!(control_specialist_due(16, 1));
    }

    #[test]
    fn national_units_take_a_bounded_share_not_the_entire_factory() {
        assert!(!signature_due(ObjectCategory::Vehicle, 3, 0));
        assert!(signature_due(ObjectCategory::Vehicle, 5, 0));
        assert!(!signature_due(ObjectCategory::Vehicle, 5, 1));
        assert!(signature_due(ObjectCategory::Vehicle, 10, 1));
        assert!(signature_due(ObjectCategory::Aircraft, 0, 0));
        assert!(!signature_due(ObjectCategory::Aircraft, 1, 1));
        assert!(signature_due(ObjectCategory::Aircraft, 2, 1));
    }

    #[test]
    fn retail_country_specials_are_not_generic_allied_actors() {
        let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules() else {
            return;
        };
        for (unit, country) in [
            ("TNKD", "Germans"), ("SNIPE", "British"),
            ("BEAG", "Alliance"), ("GTGCAN", "French"),
        ] {
            let ty = rules.object(unit).unwrap_or_else(|| panic!("{unit}"));
            assert!(build::country_special(ty, country),
                "{unit} should be identified as {country}-exclusive");
            assert!(!build::country_special(ty, "Americans"),
                "{unit} is not an American generic unit");
        }
    }
}

#[cfg(all(test, tests_sim_p_s))]
mod v30_emergency_counter_tests {
    use super::*;

    #[test]
    fn retail_vehicle_array_kirov_is_an_airborne_counter_target() {
        let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules()
        else { return; };
        let kirov = rules.object("ZEP").expect("balloon hover bomber");
        assert_eq!(kirov.category, ObjectCategory::Vehicle);
        assert_eq!(threat_class(&rules, kirov), Some(ThreatClass::Airborne));
        let seal = rules.object("GHOST").expect("navy seal");
        assert_eq!(threat_class(&rules, seal), Some(ThreatClass::Saboteur));
        let demo = rules.object("DTRUCK").expect("demolition truck");
        assert_eq!(threat_class(&rules, demo), Some(ThreatClass::Explosive));
    }
}
