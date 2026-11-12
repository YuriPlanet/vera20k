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
use crate::rules::unit_role::ROLES;
use crate::sim::control::brain::Outbox;
use crate::sim::control::intent::Intent;
use crate::sim::control::observation::Observation;
use crate::sim::intern::InternedId;
use crate::sim::production::ProductionCategory;

use super::RetailBrain;
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

/// The price of the cheapest line vehicle the house can build now.
///
/// Two questions read it, and they are the same question from two sides: what
/// the barracks is saving the wallet for, and what the war factory has to be
/// able to pay before it has anything to make. It is read from the rules
/// rather than fixed at [`COMBAT_MIN_CREDITS`]: the fixed floor is the price
/// of a *heavy* unit, and a war factory held to it makes nothing while the
/// barracks spends every credit above its own floor — the army that is all
/// infantry and never a tank. A side whose line tank is dearer than another's
/// answers with its own price this way.
fn line_vehicle_floor(rules: &RuleSet, obs: &Observation<'_>) -> Option<i32> {
    build::class_types(rules, obs, ObjectCategory::Vehicle)
        .filter(|(ty, _)| super::orders::is_combat_unit(rules, ty))
        .map(|(ty, _)| obs.cost_of(rules, ty))
        .min()
}

/// The wallet past which the house reaches for the heavy end; under it, one
/// line unit's price is as far as it goes ([`unit_ceiling`]).
const RICH_CREDITS: i32 = 4_000;

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
    brain: &mut RetailBrain,
    rules: &RuleSet,
    obs: &Observation<'_>,
    budget: &Budget,
    out: &mut Outbox,
) {
    // What the yard is saving for is not the lines' money: it decided on a
    // structure, and paying for it is what "decided" means. Taken here, once,
    // so no line below can spend it.
    let budget = budget.reserving(brain.yard_reserve);
    // The infantry floor decides who gets the wallet first when it is thin: a
    // house below its share of infantry spends on the barracks before it
    // spends on the war factory.
    let infantry_short = infantry_below_floor(&brain.census);
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
    let vehicle_floor = line_vehicle_floor(rules, obs).unwrap_or(COMBAT_MIN_CREDITS);
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
    let infantry_full = infantry_above_ceiling(&brain.census);
    let saving_for_the_tank = budget.free < vehicle_floor;
    let a_soldier_would_reset_the_saving =
        saving_for_the_tank && budget.free >= vehicle_floor - INFANTRY_MIN_CREDITS;
    // Read for the log line and for the "save versus spend" note above; it is
    // deliberately not part of the predicate any more.
    let vehicle_affordable = budget.free >= vehicle_floor && budget.free < RICH_CREDITS;
    let _ = vehicle_affordable;
    let barracks_holds = infantry_full && !a_soldier_would_reset_the_saving;
    let order = if infantry_short {
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
        // An army that already holds its share of infantry stops buying it,
        // so the wallet can reach a vehicle — unless the wallet is so far under
        // that price that one soldier would not set the saving back. Then the
        // barracks keeps turning rather than leaving both lines idle.
        if barracks_holds && category == ProductionCategory::Infantry {
            continue;
        }
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
        let wanted = product_for(
            brain,
            rules,
            obs,
            &budget,
            category,
            infantry_short,
            vehicle_floor,
        );
        // A capturer the line starts is booked against the ledger here, at the
        // one place a unit is actually queued: a `product_for` that returned a
        // type the wallet could not pay for books nothing, because
        // `queue_unit` is the same test.
        if let Some((ty, _)) = wanted
            && budget.free >= obs.cost_of(rules, ty)
        {
            let traits = census::Traits::of(rules, ty);
            if traits.engineer || traits.spy {
                brain.engineers.note_bought(obs.tick());
            }
        }
        queue_unit(rules, obs, &budget, out, wanted);
    }
}

/// Whether the army is short of its infantry share.
///
/// A house with no army at all is short of it: `fighters.max(1)` makes the
/// first soldier the floor's, rather than leaving an empty army's share at
/// zero and letting the war factory decide what the opening is made of.
fn infantry_below_floor(census: &Census) -> bool {
    let infantry: usize = census.tiers[Slot::Infantry.index()].iter().sum();
    infantry * 100 < census.fighters.max(1) * INFANTRY_SHARE_PERCENT
}

/// Whether the army has all the infantry it needs, so the wallet can go to the
/// vehicles instead ([`INFANTRY_CEILING_PERCENT`]).
///
/// The same reading the floor makes, of the same fighters: a house with no
/// army is below the floor and below the ceiling, and the barracks fires.
fn infantry_above_ceiling(census: &Census) -> bool {
    let infantry: usize = census.tiers[Slot::Infantry.index()].iter().sum();
    infantry * 100 > census.fighters.max(1) * INFANTRY_CEILING_PERCENT
}

/// How many engineers the house keeps, and how many spies.
///
/// Two and one. An engineer is spent the moment it walks into a building, so
/// the house wants another behind it — a captured refinery is worth a dozen
/// soldiers, and the second engineer is the one that takes it after the first
/// is gone. A spy is a raid that walks for minutes and is worth exactly one at
/// a time: it does its work in a single building and does not come out.
const ENGINEER_WANT: usize = 2;
const SPY_WANT: usize = 1;

/// How many attack dogs the house keeps: one for the scan, a second behind it
/// for the dog the enemy sends at the first.
const DOG_WANT: usize = 2;

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
    /// Engineers and spies bought, as the queue saw them.
    pub(super) bought: u32,
    /// Buildings taken: a capture that landed.
    pub(super) captured: u32,
    /// The frame the house stops buying until, once the ratio fails.
    pub(super) quiet_until: u64,
}

impl EngineerLedger {
    /// Whether the house may buy another engineer or spy this frame.
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
        // Judged after the increment, on the purchase that was just made: one
        // engineer bought and none taken is not yet a failure, four per
        // building taken is.
        let patience_exceeded = self.bought > self.captured.saturating_mul(ENGINEER_WASTE_PATIENCE);
        if patience_exceeded {
            self.quiet_until = tick.saturating_add(ENGINEER_QUIET_FRAMES);
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
    let want_engineer = engineers < ENGINEER_WANT && may_buy_capturers;
    let want_spy = spies < SPY_WANT && may_buy_capturers;
    let want_dog = dogs < DOG_WANT;
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
                u8::from(!traits.scout),
                u8::from(!traits.engineer),
                obs.cost_of(rules, ty),
                *id,
            )
        })
        .map(|(ty, id, _)| (ty, id))
}

/// What one idle line starts next.
fn product_for<'r>(
    brain: &mut RetailBrain,
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
    if let Some(found) = chosen_product(rules, obs, category, budget.free) {
        return Some(found);
    }

    if category == ProductionCategory::Vehicle {
        // A yard maker comes first while the house has neither a yard nor one
        // on the field — everything else the base builds is gated on it.
        let needs_maker = !super::expand::yard_stands(&brain.census)
            && !super::expand::owns_yard_maker(&brain.census);
        if needs_maker && let Some(maker) = super::expand::yard_maker(rules, obs) {
            return Some(maker);
        }
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
        && let Some(found) = tool(rules, obs, &brain.census, budget.free, &brain.engineers)
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
            // An aircraft line is worth running at the same floor as the
            // vehicles: it is the dearest unit the house can buy.
            if budget.free < COMBAT_MIN_CREDITS {
                return None;
            }
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

/// The type the house's own choice has this factory make, when the house may
/// still build it and pay for it: `Suggest_New_Object 0x004FBD80`.
///
/// Two guards, both the caller's, because the native reader is unchecked: the
/// type must still be buildable (a choice can outlive its factory or its
/// prerequisite) and affordable. A harvester is skipped too — the harvester
/// count above is the economy's own owner, and the slot can still name one
/// after the count is met.
fn chosen_product<'r>(
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
        || obs.can_build(rules, ty, true) == crate::sim::production::CanBuild::No
        || credits < obs.cost_of(rules, ty)
    {
        return None;
    }
    Some((ty, type_id))
}

/// The fighting type of `category` the line should start next.
///
/// The gap closes first: a type is what it brings to a job the army cannot
/// already answer. Only when nothing is short does the price band decide, and
/// then the type the house holds fewest of — so a mixed army forms instead of
/// one type forever.
fn fighter<'r>(
    brain: &mut RetailBrain,
    rules: &'r RuleSet,
    obs: &Observation<'_>,
    budget: &Budget,
    category: ObjectCategory,
    naval: Option<bool>,
    infantry_short: bool,
) -> Option<(&'r ObjectType, InternedId)> {
    let ceiling = unit_ceiling(budget.free);
    let affordable: Vec<(&ObjectType, InternedId, i32)> = build::class_types(rules, obs, category)
        .filter(|(ty, _)| naval.is_none_or(|wanted| ty.naval == wanted))
        .filter(|(ty, _)| super::orders::is_combat_unit(rules, ty))
        .filter_map(|(ty, type_id)| {
            let cost = obs.cost_of(rules, ty);
            (cost <= ceiling).then_some((ty, type_id, cost))
        })
        .collect();
    if affordable.is_empty() {
        return None;
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
    budget: &Budget,
    out: &mut Outbox,
    wanted: Option<(&ObjectType, InternedId)>,
) {
    let Some((ty, type_id)) = wanted else {
        return;
    };
    if budget.free < obs.cost_of(rules, ty) {
        return;
    }
    log::debug!(
        "auto play: unit {} at {} (credits {})",
        ty.id,
        obs.cost_of(rules, ty),
        budget.free
    );
    out.intent(Intent::QueueProduction { type_id });
}

#[cfg(test)]
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

        let mut brain = RetailBrain::new(0xA17);
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

        let mut brain = RetailBrain::new(0xA17);
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

        let mut brain = RetailBrain::new(0xA17);
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

        let mut brain = RetailBrain::new(0xA17);
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

        let mut brain = RetailBrain::new(0xA17);
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

        let mut brain = RetailBrain::new(0xA17);
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
    fn a_barracks_keeps_turning_while_the_wallet_is_below_the_war_factory() {
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
        let floor = line_vehicle_floor(&rules, &obs).expect("the side has a line vehicle");
        let under = floor.saturating_sub(100).max(INFANTRY_MIN_CREDITS);
        sim.houses
            .get_mut(&owner)
            .expect("the house")
            .economy_mut()
            .set_credits_for_test(under);

        let mut brain = RetailBrain::new(0xA17);
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
            queued
                .iter()
                .any(|name| rules.object(name).expect("a rules type").category
                    == ObjectCategory::Infantry),
            "the base does not stand idle: {queued:?}"
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

        let mut brain = RetailBrain::new(0xA17);
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
        let (ty, _) = tool(&rules, &obs, &census, 100_000, &EngineerLedger::default())
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

        let mut brain = RetailBrain::new(0xA17);
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
        assert!(
            !sunk.may_buy(0),
            "ten bought for two taken is 狂造工程师拖垮经济"
        );
        // And quiet is a window, not a death sentence: the front moves.
        assert!(sunk.may_buy(ENGINEER_QUIET_FRAMES));
    }
}
