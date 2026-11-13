//! The build list, and the wallet that pays for it.
//!
//! What the base builds is one ordered list, read top to bottom, and what it
//! may spend is the wallet minus what the lines have already been promised.
//! Those were one question before this module: the order was a chain of
//! conditions, each one reading the wallet of the moment, and a queue is
//! money the wallet has not been charged for yet — a build starts owing its
//! `Cost_Of` and pays it down as it steps (`sim::production::factory`), so
//! queuing costs the house nothing today. The same credits were therefore
//! spent twice: a structure was started with money the army had already been
//! promised, and the wallet never got to the price of a tank.
//!
//! Three numbers come out of here:
//!
//! - **What the house earns per minute**, measured: two samples of
//!   [`Observation::earned`] a window apart. It is what turns the list into a
//!   budget — a structure that costs a minute's income is a decision, not a
//!   reflex, and the log line the brain writes every ten seconds says how the
//!   house is doing rather than only what it holds.
//! - **What is still free**: the wallet minus [`Census::committed`], the
//!   money no line has been promised yet. Every gate below reads that and
//!   none of them reads the balance.
//! - **Whether the base is short of power**, counting the plants still
//!   building at the output they will have when they finish
//!   ([`power_short`]). A plant under construction supplies its `Power=`
//!   scaled by the health it has built, while everything it feeds draws in
//!   full — read naively that is a brownout for as long as the plant is
//!   being built, and the answer it produced was a second plant: the opening
//!   became two plants and one refinery.
//!
//! Determinism: integer arithmetic over the rules' arrays in their own order,
//! and the meter is a ring of samples keyed by the frame they were taken at,
//! so the same replay gives the same rate.

use std::collections::VecDeque;

use crate::rules::object_type::ObjectType;
use crate::rules::ruleset::RuleSet;
use crate::sim::control::observation::Observation;
use crate::sim::intern::InternedId;
use crate::sim::production::ProductionCategory;

use super::build;
use super::census::Census;
use super::expand;

/// A minute at play cadence: the simulation steps 15 logic frames a second,
/// the rate every retail mode runs at, so this is the frame count a per-minute
/// figure is scaled over.
const FRAMES_PER_MINUTE: u64 = 900;

/// How far apart the meter takes its samples: three seconds. Close enough
/// that a rate is current, far enough that the ring stays short.
const SAMPLE_FRAMES: u64 = 45;

/// The shortest window the meter will extrapolate from: ten seconds. Under it
/// one deposit — or no deposit — is the entire sample, and a rate read off
/// that swings by thousands.
const MIN_WINDOW_FRAMES: u64 = 150;

/// How much history the meter keeps: one minute. A rate older than that is a
/// rate from a base that no longer stands.
const WINDOW_FRAMES: u64 = FRAMES_PER_MINUTE;

/// The house's income, measured rather than modelled.
///
/// A model would need the distance from the base to the ore, the harvester's
/// speed and how long a bay takes to unload — every one of them map- and
/// type-specific, and the answer is wrong in exactly the cases that matter (a
/// field that is running out, a bay that is too far away, a harvester that was
/// killed). The house's own books are simpler and always right: what it has
/// earned is the wallet plus everything `Spend_Money` has taken out of it.
pub(super) struct IncomeMeter {
    /// `(frame, earned)` samples, oldest first.
    samples: VecDeque<(u64, i32)>,
}

impl IncomeMeter {
    pub(super) fn new() -> Self {
        Self {
            samples: VecDeque::new(),
        }
    }

    /// Take a sample of `earned` at `frame`, if the last one is old enough.
    pub(super) fn sample(&mut self, frame: u64, earned: i32) {
        if self
            .samples
            .back()
            .is_some_and(|(at, _)| frame.wrapping_sub(*at) < SAMPLE_FRAMES)
        {
            return;
        }
        self.samples.push_back((frame, earned));
        while self.samples.len() > 2
            && frame.wrapping_sub(self.samples.front().map_or(0, |(at, _)| *at)) > WINDOW_FRAMES
        {
            self.samples.pop_front();
        }
    }

    /// Credits per minute over the window the meter holds, or `None` before
    /// the window is long enough to extrapolate from.
    pub(super) fn per_minute(&self) -> Option<i32> {
        let (first_frame, first_earned) = *self.samples.front()?;
        let (last_frame, last_earned) = *self.samples.back()?;
        let frames = last_frame.wrapping_sub(first_frame);
        if frames < MIN_WINDOW_FRAMES {
            return None;
        }
        let earned = i64::from(last_earned.wrapping_sub(first_earned));
        let window = i64::try_from(frames).unwrap_or(i64::MAX);
        let minute = i64::try_from(FRAMES_PER_MINUTE).unwrap_or(i64::MAX);
        i32::try_from(earned * minute / window).ok()
    }
}

/// What the wallet can actually spend this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Budget {
    /// What the lines have been promised and have not paid out yet.
    pub(super) committed: i32,
    /// The wallet minus the commitment.
    pub(super) free: i32,
    /// Credits per minute, when the meter has a window.
    pub(super) income: Option<i32>,
}

impl Budget {
    /// The same budget with `credits` held back.
    ///
    /// The yard reserves what it is saving for ([`super::build`]), and a
    /// factory that spends it is a house that never finishes the structure it
    /// decided on. One owner for "how much may the lines spend", so a caller
    /// cannot reserve money one place and spend it in another.
    pub(super) fn reserving(&self, credits: i32) -> Self {
        Self {
            committed: self.committed,
            free: self.free.saturating_sub(credits.max(0)),
            income: self.income,
        }
    }
}

/// Read the budget: the ledger's own commitment, and the wallet beside it.
pub(super) fn budget(
    rules: &RuleSet,
    obs: &Observation<'_>,
    census: &Census,
    income: Option<i32>,
) -> Budget {
    let committed = census.committed(rules, obs);
    Budget {
        committed,
        free: obs.credits().wrapping_sub(committed),
        income,
    }
}

/// One kind of structure the list asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Item {
    /// Not an entry of [`STEPS`]: power is spliced in ahead of the list, the way
    /// native `AI_Choose_Building` (`0x004FE6F6..0x004FE943`) does it, because a
    /// brownout outranks every plan. It is a variant here so a spliced plant can
    /// be named by the same answer type the list's own entries are.
    Power,
    Refinery,
    Barracks,
    Weapons,
    Defense,
    Radar,
    Tech,
    /// The plant an `Aircraft` line needs, read off `Factory=` rather than off
    /// a name list.
    Aircraft,
    /// The `OrePurifier=` structure (`[GAOREP]`): every load the bays take is
    /// worth more while one stands. Read off the rules' own flag, so a mod's
    /// purifier is answered by the same query.
    Purifier,
    /// The `FactoryPlant=` structure (`[NAINDP]`): `UnitsCostBonus=0.75` is a
    /// quarter off every vehicle for 2500 credits, which pays for itself the
    /// moment the house has a war factory running. Read off the flag, not the
    /// name: the industrial plant is a Soviet structure in retail and a mod may
    // put the key on something else.
    IndustrialPlant,
    /// The `SpySat=yes` structure (`[GASPYSAT]`): the whole map's radar, which
    /// the player can see. `[AI] AIBuildThis=no` says the *computer* does not
    /// build it — but auto play is a hand on the player's own base, and a
    /// revealed map is worth more to a player than to the AI that already has
    /// the map. So this step exists where the native AI's does not.
    SpySatellite,
    /// The `UnitRepair=` shed (`[GADEPT] UnitRepair=yes`): where a hurt vehicle
    /// stops being a loss. Without one the retreat has nowhere to send a
    /// damaged unit, and the army shrinks by every fight it survives.
    Repair,
    /// What the yard maker's `Prerequisite=` list asks for: the way back from
    /// a lost yard.
    Support,
    /// A building that grants a power which changes the map (`SuperWeapon=`,
    /// `NukeSilo=`): the plot a house buys once its wallet outruns its lines.
    Superweapon,
}

/// When a step of the list is allowed to answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Gate {
    Always,
    /// The bays have the harvesters that keep them busy
    /// ([`build::HARVESTERS_PER_REFINERY`]): a second bay before the first
    /// one's harvesters is a bay with nothing to unload.
    HarvestersServed,
    /// The free wallet is past this. Throughput and tech are luxuries the base
    /// pays for out of a surplus, never out of the army's money.
    Free(i32),
    /// The house is earning more than its lines can spend: a wallet that is big
    /// once is luck ([`Gate::Free`]), an income the factories cannot turn into
    /// army is a bottleneck, and a bottleneck is the honest reason to buy
    /// throughput.
    Surplus,
    /// The base can replace what a push spends: a barracks and a war factory
    /// both stand.
    CanReplace,
    /// The tech stage: a surplus and an army worth unlocking types for.
    Tech,
}

/// One entry of the build list: what is wanted, how many, and when.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Step {
    pub(super) item: Item,
    pub(super) want: usize,
    pub(super) gate: Gate,
}

/// The list, in the order a player builds a base.
///
/// Power is not an entry: it is spliced in ahead of whatever the list answers
/// ([`power_short`]), the way a player does it, because a plant is not a step
/// towards anything — it is what lets the other steps run.
pub(super) const STEPS: &[Step] = &[
    // Power first, as a step rather than only as the splice below. The retail
    // prerequisites say the same thing: `[GAPOWR]` needs nothing but the yard,
    // and every structure after it has `POWER` in its list — `[GAREFN]` most
    // explicitly. Naming it is also what makes the opening one plant: a plant
    // that is only spliced in when something else would brown the base is a
    // plant the base buys twice.
    Step {
        item: Item::Power,
        want: 1,
        gate: Gate::Always,
    },
    // The soldiers first: the barracks is the cheaper of the two, it is what a
    // dog comes out of — the only early answer to an enemy engineer walking
    // into the base — and it is what an engineer comes out of, and an engineer
    // is the one unit that pays for itself on the map's own buildings.
    Step {
        item: Item::Barracks,
        want: 1,
        gate: Gate::Always,
    },
    // Then the income. A bay with a truck at it is the whole of what the house
    // earns, and every step below this one is paced by the wallet.
    Step {
        item: Item::Refinery,
        want: 1,
        gate: Gate::Always,
    },
    // The war factory is part of the tech chain, not a throughput luxury: the
    // lab's own `Prerequisite=GAWEAP,RADAR,GACNST`, and the harvester the
    // second bay needs comes out of it.
    Step {
        item: Item::Weapons,
        want: 1,
        gate: Gate::Always,
    },
    // Then tech, as fast as the wallet allows: the plant an aircraft line
    // needs (which is also the Americans' radar — `[AMRADR]
    // Factory=AircraftType,Radar=yes`), the radar, and the lab the rest of the
    // tree hangs off (`[GAOREP] Prerequisite=GATECH,...`).
    //
    // Ahead of the defenses and of every second line, because the list answers
    // with the first step that is short and open: with a tower ahead of them
    // the house bought towers out of the money the lab needed, and the tech
    // stage never came.
    Step {
        item: Item::Aircraft,
        want: 1,
        gate: Gate::Tech,
    },
    Step {
        item: Item::Radar,
        want: 1,
        gate: Gate::Tech,
    },
    Step {
        item: Item::Tech,
        want: 1,
        gate: Gate::Tech,
    },
    // The shed that puts a damaged vehicle back in the line, once the tree is
    // open: a fight the army survives is still a fight it cannot repeat while
    // every hurt tank is parked for good. Ahead of the towers because a base
    // with no repair is a base whose army only ever gets smaller.
    Step {
        item: Item::Repair,
        want: 1,
        gate: Gate::CanReplace,
    },
    // The second and third bays before the towers and the extra lines: the ore
    // a bay unloads is what pays for everything below it, and a house that
    // reaches the midgame on one bay earns a third of what it could.
    Step {
        item: Item::Refinery,
        want: 3,
        gate: Gate::HarvestersServed,
    },
    // The purifier, once the bays are there to feed it: `[GAOREP]` costs 2500
    // and is worth it only against three bays' worth of ore. After the refineries
    // rather than before, because a purifier pays a percentage of an income that
    // does not exist yet.
    Step {
        item: Item::Purifier,
        want: 1,
        gate: Gate::Surplus,
    },
    // The plant that takes a quarter off every vehicle. After the lab, because
    // `Prerequisite=NATECH,PROC,NACNST` puts it behind the whole tree, and
    // behind a surplus because 2500 is the army's money until the war factory
    // can spend it on something cheaper.
    Step {
        item: Item::IndustrialPlant,
        want: 1,
        gate: Gate::Surplus,
    },
    // The spy satellite, which the native computer does not build and a player
    // would: the whole map on the radar. After the tree, because it needs the
    // lab, and behind a surplus because 1500 is an army's worth.
    Step {
        item: Item::SpySatellite,
        want: 1,
        gate: Gate::Surplus,
    },
    // Ground to stand on, once the tree is open: before that the same money
    // unlocks the rest of the tree, which is worth more than a second tower.
    //
    // Six, not two. Two towers is a 1v1's ring; seven houses send seven times
    // the raids, and the towers are the only exchange that costs the house
    // nothing but credits — measured, a 1v7 with two towers lost its war
    // factory and barracks in the first ten minutes, stopped replacing its
    // army, and its fighters fell from 95 to 10.
    Step {
        item: Item::Defense,
        want: 2,
        gate: Gate::CanReplace,
    },
    // Throughput, once the wallet says the factories cannot spend it. Every
    // extra factory shortens a build by `[General] MultipleFactory=`.
    Step {
        item: Item::Weapons,
        want: 3,
        gate: Gate::Surplus,
    },
    Step {
        item: Item::Barracks,
        want: 2,
        gate: Gate::Surplus,
    },
    // The way back from a lost yard, bought while the yard still stands: last,
    // because it is insurance against a loss the house has not had.
    Step {
        item: Item::Support,
        want: 1,
        gate: Gate::Free(build::SUPPORT_MIN_CREDITS),
    },
    // The plot, once there is money the lines cannot spend: a superweapon is
    // the one purchase that takes a base apart, and a house that owns one wins
    // the fight it was losing. Last, because nothing below it needs it and a
    // strike building before an army is a base that dies with the plot half
    // built.
    Step {
        item: Item::Superweapon,
        want: 1,
        gate: Gate::Surplus,
    },
];

/// The list's answer, and the entry of the list it came from.
pub(super) struct Chosen<'r> {
    /// The index of the step in [`STEPS`], which is what a hold names: an item
    /// appears more than once (the first war factory is a `want` of one and the
    /// second a `want` of three), and holding "a war factory" would hold the
    /// wrong one. `None` for the spliced power plant, which is no step at all.
    pub(super) step: Option<usize>,
    pub(super) item: Item,
    pub(super) ty: &'r ObjectType,
    pub(super) type_id: InternedId,
}

/// The structure the list wants now, keeping the one it wanted before while
/// `held` still stands.
///
/// A gate that opens on the wallet closes again as soon as the wallet is spent
/// — [`Gate::Surplus`] is the obvious one — so a list that re-decides every
/// frame answers a different entry on every dip: the surplus for a second
/// factory became a defense the moment the wallet fell under the line, and the
/// factory was never bought. A house does not do that. It decides and then
/// saves for the thing it decided on, and that is what `held` is: the same
/// entry, for as long as it is still short, whatever the wallet does in
/// between.
///
/// Power is not held: it is spliced in ahead of this by the caller, because a
/// brownout outranks every plan the house has.
pub(super) fn answer<'r>(
    rules: &'r RuleSet,
    obs: &Observation<'_>,
    census: &Census,
    budget: &Budget,
    held: Option<usize>,
) -> Option<Chosen<'r>> {
    // A held entry that is no longer short, or that the house can no longer
    // build, falls through to the list: the hold is a promise to save for
    // something, not a promise to wait for something that is already there.
    //
    // The gate is deliberately not asked: the whole reason for the hold is
    // that the gate has closed with the wallet that opened it.
    if let Some(step) = held
        && let Some(entry) = STEPS.get(step)
        && owned(rules, obs, census, entry.item) < want(entry, census)
        && let Some((ty, type_id)) = pick(rules, obs, census, entry.item)
    {
        return Some(Chosen {
            step: Some(step),
            item: entry.item,
            ty,
            type_id,
        });
    }
    STEPS
        .iter()
        .enumerate()
        .filter(|(_, step)| open(step.gate, rules, obs, census, budget))
        .filter(|(_, step)| owned(rules, obs, census, step.item) < want(step, census))
        .find_map(|(step, entry)| {
            pick(rules, obs, census, entry.item).map(|(ty, type_id)| Chosen {
                step: Some(step),
                item: entry.item,
                ty,
                type_id,
            })
        })
}

/// How many of `step`'s item the base wants, which for towers is a question
/// about the war and not about the plan.
///
/// Two towers is a 1v1's ring: one house sends the raid the two of them meet.
/// Seven houses send seven times the raiding, and the towers are the only
/// exchange that costs the house nothing but credits — measured, a 1v7 with
/// two towers lost its production in the first ten minutes, stopped replacing
/// its army, and its fighters fell from 95 to 10 while the enemy's count did
/// not move. Every twelve hostiles the house can see is one more tower.
fn want(step: &Step, census: &Census) -> usize {
    match step.item {
        Item::Defense => step.want + census.enemies.len() / 12,
        _ => step.want,
    }
}

/// Whether the base needs another plant, counting the plants that are still
/// building at the output they will have when they finish.
/// How much power the base is short of, in `Power=` units; negative or zero
/// means it has enough.
///
/// One number instead of the two tests the yard used to ask. What it adds is
/// the draw the house is about to take on: a lab at `-100` is a plant the base
/// needs *before* it starts the lab, not after it is dark. And what it takes
/// away is the second plant — a plant already on a line, finished and waiting
/// for a cell, or standing unfinished is output the base is already paying for,
/// so the deficit it covers is covered, and the yard does not buy the same
/// output twice.
///
/// The dead band is the number itself: buying one plant drops the deficit by
/// that plant's `Power=` the same frame it is queued, so a house cannot queue a
/// second one for the same shortage.
pub(super) fn power_deficit(
    rules: &RuleSet,
    obs: &Observation<'_>,
    census: &Census,
    about_to_draw: i32,
) -> i32 {
    obs.power_drain()
        .saturating_add(about_to_draw.abs().min(i32::MAX - 1))
        .wrapping_sub(planned_output(rules, obs, census))
}

pub(super) fn power_short(rules: &RuleSet, obs: &Observation<'_>, census: &Census) -> bool {
    power_deficit(rules, obs, census, 0) > 0
}

/// The output the base has once everything it is already paying for is built.
///
/// A plant under construction supplies `Power=` scaled by the health it has
/// built (`sim::power_system`), a plant that has finished but has not been
/// placed supplies nothing yet, and one still on the yard's line supplies
/// nothing either. All three are counted here at what they *will* supply,
/// because the question is not "is the base dark this frame" — the
/// simulation's own answer to that is the brownout — but "does the base need
/// another plant", which is a question about the plant it is already paying
/// for.
///
/// The sum is taken from the buildings themselves rather than from
/// [`Observation::power_output`], because the running figure is the scaled one:
/// a plant at a tenth of its health reads as a tenth of its output, which is
/// exactly the deficit that answered "build a second plant" while the first was
/// still going up. That was the opening with two plants and one refinery, and
/// it survived the earlier fixes because the plant was no longer on a line by
/// the time it was counted.
pub(super) fn planned_output(rules: &RuleSet, obs: &Observation<'_>, census: &Census) -> i32 {
    let mut output: i32 = 0;
    for entity in obs.own_entities() {
        let Some(ty) = obs.object_type(rules, entity.type_id) else {
            continue;
        };
        if ty.category != crate::rules::object_type::ObjectCategory::Building {
            continue;
        }
        output = output.wrapping_add(ty.power.max(0));
    }
    // What the yard holds and has not put down yet: it is on the map as far as
    // this question is concerned, and on no line, so the loop above cannot see
    // it.
    for line in &census.lines {
        for type_id in [line.product, line.ready].into_iter().flatten() {
            if let Some(ty) = obs.object_type(rules, type_id) {
                output = output.wrapping_add(ty.power.max(0));
            }
        }
    }
    output
}

fn open(
    gate: Gate,
    rules: &RuleSet,
    obs: &Observation<'_>,
    census: &Census,
    budget: &Budget,
) -> bool {
    match gate {
        Gate::Always => true,
        Gate::HarvestersServed => census.harvesters_served(),
        Gate::Free(credits) => budget.free >= credits,
        Gate::Surplus => {
            budget.free >= build::SURPLUS_CREDITS
                || budget
                    .income
                    .is_some_and(|rate| rate >= build::SURPLUS_INCOME_PER_MINUTE)
        }
        Gate::CanReplace => build::can_replace_units(rules, obs, census),
        // The wallet, and only the wallet: a house that waits to have an army
        // before it unlocks the tree is a house that pays for the whole army
        // with the opening's tech level. `build_next` already refuses a
        // structure the wallet cannot cover, and the threshold here is the
        // cheapest thing the stage buys ([`build::TECH_START_CREDITS`]): the
        // stage is decided there, and what it still needs is held back from
        // the lines rather than waited for.
        Gate::Tech => budget.free >= build::TECH_START_CREDITS,
    }
}

/// How many of `item` the house has.
fn owned(rules: &RuleSet, obs: &Observation<'_>, census: &Census, item: Item) -> usize {
    match item {
        Item::Power => build::count_in(obs, rules, census, &rules.build_power_types),
        Item::Refinery => census.refineries,
        Item::Purifier => build::count_ore_purifiers(rules, obs),
        Item::IndustrialPlant => build::count_industrial_plants(rules, obs),
        Item::SpySatellite => build::count_spy_satellites(rules, obs),
        Item::Barracks => build::count_in(obs, rules, census, &rules.build_barracks_types),
        Item::Weapons => build::count_in(obs, rules, census, &rules.build_weapons_types),
        Item::Defense => build::defenses(rules, obs, census),
        Item::Radar => build::count_in(obs, rules, census, &rules.build_radar_types),
        Item::Tech => build::count_in(obs, rules, census, &rules.build_tech_types),
        // A house that owns a plant of the category has nothing to build, and
        // one that cannot build any has no business asking: both read as
        // owned, which is the answer that stops the step repeating.
        Item::Aircraft => usize::from(
            build::line_factory(rules, obs, census, ProductionCategory::Aircraft).is_none(),
        ),
        // The support building is the yard maker's own prerequisite list, so
        // whether one is wanted is that list's answer, not a count's.
        Item::Support => 0,
        // One shed is enough: it repairs a whole army, one unit at a time.
        Item::Repair => usize::from(census.repair_building.is_some()),
        Item::Superweapon => build::superweapons(rules, obs, census),
    }
}

fn pick<'r>(
    rules: &'r RuleSet,
    obs: &Observation<'_>,
    census: &Census,
    item: Item,
) -> Option<(&'r ObjectType, InternedId)> {
    match item {
        Item::Power => build::first_buildable(rules, obs, &rules.build_power_types),
        Item::Refinery => build::first_buildable(rules, obs, &rules.build_refinery_types),
        Item::Purifier => build::ore_purifier_building(rules, obs),
        Item::IndustrialPlant => build::industrial_plant_building(rules, obs),
        Item::SpySatellite => build::spy_satellite_building(rules, obs),
        Item::Barracks => build::first_buildable(rules, obs, &rules.build_barracks_types),
        Item::Weapons => build::first_buildable(rules, obs, &rules.build_weapons_types),
        Item::Defense => build::defense_building(rules, obs, census),
        Item::Radar => build::first_buildable(rules, obs, &rules.build_radar_types),
        // The lab that authorises a power the house will actually fire.
        Item::Tech => build::tech_building(rules, obs),
        Item::Aircraft => build::line_factory(rules, obs, census, ProductionCategory::Aircraft),
        Item::Support => expand::yard_maker_support(rules, obs, census),
        Item::Repair => build::unit_repair_building(rules, obs),
        Item::Superweapon => build::superweapon_building(rules, obs),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::sim::world::Simulation;

    /// The retail rival arena: real object types, from the repository's own
    /// `ini/` snapshot, so this holds without a retail install.
    fn arena(seed: u64) -> Option<(Simulation, RuleSet)> {
        let rules = crate::rules::retail_ini_fixture::retail_unlayered_rules()?;
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, seed);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        Some((sim, rules))
    }

    /// Two samples ten seconds apart, ten credits earned in between: that is
    /// sixty a minute. The wallet alone would have read it as nothing — the
    /// ten credits went straight back out to the line that was building.
    #[test]
    fn the_meter_reads_income_the_wallet_does_not_show() {
        let mut meter = IncomeMeter::new();
        // 0 s: 1000 in the wallet, 0 paid out.
        meter.sample(0, 1_000);
        assert_eq!(meter.per_minute(), None, "one sample is not a rate");
        // 10 s: the wallet is unchanged, but 100 has been spent, so the house
        // earned 100.
        meter.sample(150, 1_100);
        assert_eq!(meter.per_minute(), Some(600));
    }

    /// A window older than a minute is dropped: a rate from a base that no
    /// longer stands is not the house's income now.
    #[test]
    fn the_meter_forgets_more_than_a_minute() {
        let mut meter = IncomeMeter::new();
        meter.sample(0, 0);
        meter.sample(2_000, 2_000);
        let rate = meter.per_minute().expect("a rate");
        assert!(
            rate < 1_000,
            "the oldest sample is gone, so the rate is over the last minute at most: {rate}"
        );
    }

    /// What a line has been promised is spent already: a build starts owing
    /// its `Cost_Of` and pays it down as it steps, so the wallet still holds
    /// money a decision may not spend a second time.
    #[test]
    fn a_promised_build_is_money_the_wallet_may_not_spend_again() {
        let Some((mut sim, rules)) = arena(0xA17) else {
            return;
        };
        let owner = sim.interner.intern("Americans");
        let mut census = Census::default();
        let plant = sim.interner().get("GAPOWR").expect("the plant's id");
        census.lines[crate::sim::strategy::retail::census::LINE_BUILDING].product = Some(plant);
        let obs = Observation::new(&sim, owner);
        let ty = rules.object("GAPOWR").expect("the plant");

        let budget = budget(&rules, &obs, &census, None);
        assert_eq!(budget.committed, obs.cost_of(&rules, ty));
        assert_eq!(budget.free, obs.credits().wrapping_sub(budget.committed));
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

    /// A base that has what the opening asks for and nothing spare: the second
    /// war factory is not open to it, because the gate that buys throughput
    /// answers a surplus and this house has none.
    fn opening_base(seed: u64) -> Option<(Simulation, RuleSet, InternedId)> {
        let (mut sim, rules) = arena(seed)?;
        let owner = sim.interner.intern("Americans");
        for (name, cell) in [
            ("GACNST", (16u16, 16u16)),
            ("GAPOWR", (12, 16)),
            ("GAREFN", (20, 20)),
            ("GAPILE", (24, 24)),
            ("GAWEAP", (28, 28)),
        ] {
            sim.spawn_object(name, "Americans", cell.0, cell.1, 255, &rules)
                .unwrap_or_else(|| panic!("{name}"));
        }
        sim.houses
            .get_mut(&owner)
            .expect("the house")
            .economy_mut()
            .set_credits_for_test(2_000);
        Some((sim, rules, owner))
    }

    /// A gate that opens on the wallet closes again as soon as the wallet is
    /// spent. The list is therefore not asked afresh every frame: the entry it
    /// decided on is held, and answered even though its own gate has shut —
    /// that is the house saving for what it chose instead of spending the
    /// savings on whichever entry a flickering gate answers next.
    /// A plant the yard is already paying for is output the base has, so the
    /// shortage it covers is covered: the deficit is what makes one plant one
    /// plant.

    /// A `UnitRepair=` shed is what makes a hurt vehicle a unit again, and the
    /// retreat has been clicking one since B1b. Without a step it only arrived
    /// as the yard maker's prerequisite, late and by accident — the report of
    /// a base with damaged tanks and no depot at all.
    #[test]
    fn the_service_depot_is_a_step_and_comes_before_the_towers() {
        let repair = STEPS
            .iter()
            .position(|step| step.item == Item::Repair)
            .expect("a repair depot is a step");
        let towers = STEPS
            .iter()
            .position(|step| step.item == Item::Defense)
            .expect("the towers are a step");
        assert!(repair < towers, "repair {repair} before towers {towers}");
        assert_eq!(STEPS[repair].want, 1, "one shed repairs a whole army");
        assert_eq!(STEPS[repair].gate, Gate::CanReplace);
    }

    /// The second bay is income, and the towers and the extra lines are not:
    /// a house that reaches the midgame on one bay earns a third of what its
    /// harvesters could unload, which is the "no miners, no money" report.
    #[test]
    fn the_second_bay_comes_before_the_towers_and_the_extra_lines() {
        let bays = STEPS
            .iter()
            .rposition(|step| step.item == Item::Refinery)
            .expect("the extra bays are a step");
        let towers = STEPS
            .iter()
            .position(|step| step.item == Item::Defense)
            .expect("the towers are a step");
        let factories = STEPS
            .iter()
            .position(|step| step.item == Item::Weapons && step.want == 3)
            .expect("the extra factories are a step");
        assert!(bays < towers, "bays {bays} before towers {towers}");
        assert!(bays < factories, "bays {bays} before factories {factories}");
    }

    /// And the step answers with the type that carries the key, not with a
    /// name: a mod's depot is the same building.
    #[test]
    fn the_repair_step_answers_with_a_unit_repair_building() {
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
        sim.spawn_object("GAPILE", "Americans", 16, 24, 255, &rules)
            .expect("a barracks");
        let census = ledger(&rules, &sim, owner);
        let obs = Observation::new(&sim, owner);
        let (ty, _) =
            pick(&rules, &obs, &census, Item::Repair).expect("a depot the base can build");
        assert!(ty.unit_repair, "the step answers UnitRepair=: {}", ty.id);
    }

    #[test]
    fn a_plant_on_the_way_is_the_output_the_base_already_has() {
        let Some((mut sim, rules)) = arena(0xA17) else {
            return;
        };
        let owner = sim.interner.intern("Americans");
        let obs = Observation::new(&sim, owner);
        let mut census = Census::default();
        // A bare base: no output, no draw, nothing short.
        assert_eq!(
            power_deficit(&rules, &obs, &census, 0),
            0,
            "nothing to power"
        );
        // A lab about to be started: its own -100 is a plant the base needs now.
        assert!(
            power_deficit(&rules, &obs, &census, -100) > 0,
            "the lab draws before it stands"
        );
        // And a plant finished and waiting for a cell is output it has.
        let plant = sim.interner().get("GAPOWR").expect("the power plant type");
        census.lines[crate::sim::strategy::retail::census::LINE_BUILDING].ready = Some(plant);
        assert!(
            power_deficit(&rules, &obs, &census, -100) <= 0,
            "the waiting plant covers the lab"
        );
    }

    /// The index in [`STEPS`] of the entry wanting `count` of `item`.
    fn step_of(item: Item, count: usize) -> usize {
        STEPS
            .iter()
            .position(|step| step.item == item && step.want == count)
            .expect("the list has that entry")
    }

    #[test]
    fn a_held_entry_answers_even_when_its_gate_has_closed() {
        let Some((sim, rules, owner)) = opening_base(0xA17) else {
            return;
        };
        let census = ledger(&rules, &sim, owner);
        let obs = Observation::new(&sim, owner);
        let budget = budget(&rules, &obs, &census, None);
        assert!(
            answer(&rules, &obs, &census, &budget, None)
                .is_none_or(|chosen| chosen.item != Item::Weapons),
            "a poor house with one war factory does not start another"
        );
        let second_factory = step_of(Item::Weapons, 3);
        let held = answer(&rules, &obs, &census, &budget, Some(second_factory))
            .expect("the hold answers the entry the house was saving for");
        assert_eq!(held.step, Some(second_factory), "the second war factory");
    }

    /// And a hold is a promise to save for something, not a promise to wait for
    /// something that already stands: an entry that is no longer short falls
    /// through to the list.
    #[test]
    fn a_hold_on_something_that_now_stands_falls_through() {
        let Some((sim, rules, owner)) = opening_base(0xA17) else {
            return;
        };
        let census = ledger(&rules, &sim, owner);
        let obs = Observation::new(&sim, owner);
        let budget = budget(&rules, &obs, &census, None);
        let first_barracks = step_of(Item::Barracks, 1);
        assert!(
            answer(&rules, &obs, &census, &budget, Some(first_barracks))
                .is_none_or(|chosen| chosen.step != Some(first_barracks)),
            "the barracks stands, and the house is done with it"
        );
    }
}
