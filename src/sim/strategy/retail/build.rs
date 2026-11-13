//! The brain's own economy: what the base builds, and in what order.
//!
//! This is **not** the retail AI's economy. `AI_EconomyStateMachine` and
//! `AI_Manage_Build_Queue` (`sim::ai_base_building`) run their credit cutoff
//! and their team accounting on `HouseClass` fields a human house never
//! fills, and a computer house is allowed money it has not earned. A house
//! this brain drives is a player's house: it earns what its harvesters
//! deliver and it pays what production charges. So the economy here is a
//! budget over the same two answers a player reads — the price
//! ([`Observation::cost_of`]) and whether the house may build it at all
//! ([`Observation::can_build`]) — and nothing is queued the wallet cannot
//! cover.
//!
//! The order is a player's, and it is written as one list rather than as
//! nested conditions, because it is the one thing about this brain a play test
//! can measure: power when the base is short of it, income, the barracks, one
//! war factory, the base's own defense, and only then throughput and tech.
//! Building types come from the `[AI]` lists in the rules (`BuildPower=`,
//! `BuildRefinery=`, `BuildBarracks=`, `BuildWeapons=`, `BuildRadar=`,
//! `BuildTech=`), so a mod's rules change what this builds without touching
//! VERA.
//!
//! The counts behind every question here come from [`super::census`], which
//! walks the house's objects once a frame; this module asks it rather than
//! walking the entity store a second time.
//!
//! Determinism: every count and every choice walks the rules' arrays and the
//! entity store in their own order, and every test is an integer comparison.

use std::collections::BTreeSet;

use crate::rules::object_type::{FactoryType, ObjectCategory, ObjectType};
use crate::rules::ruleset::RuleSet;
use crate::rules::superweapon_type::SuperWeaponKind;
use crate::sim::control::brain::Outbox;
use crate::sim::control::intent::Intent;
use crate::sim::control::observation::Observation;
use crate::sim::control::placement::{PLACEMENT_ATTEMPTS, Placement};
use crate::sim::intern::InternedId;
use crate::sim::production::{CanBuild, ProductionCategory};

use super::RetailBrain;
use super::census::Census;
use super::plan::{self, Budget, Item};

/// How many harvesters one refinery keeps busy. Two, not the rules'
/// `HarvestersPerRefinery=`: the trip out and back is most of a harvester's
/// life, and a third one mostly queues at the bay.
pub(super) const HARVESTERS_PER_REFINERY: usize = 2;

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

/// How many war factories the house runs before it is rich, and how many a
/// rich house runs: the list in [`super::plan`] holds the numbers now, because
/// they are entries of one order rather than thresholds of their own.

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

/// How long the build list's answer stands before it may answer a different
/// entry, in frames: forty seconds (600 frames at the 15 a second the
/// simulation steps).
///
/// Long enough to save for a structure the wallet cannot cover the frame it is
/// chosen — a gate opens on a wallet that is then spent elsewhere — and short
/// enough that a base which really has changed its mind (the defense it wanted
/// now stands) is not held to a plan for a minute.
pub(super) const BUILD_HOLD_FRAMES: u64 = 600;

/// How many war factories and barracks a house with a surplus runs: an entry
/// of [`super::plan::STEPS`], as the target counts above are.

/// One frame's base spending: take what is finished out of the yard and start
/// the next structure. The production lines are
/// [`super::composition::fill_lines`]' job.
pub(super) fn spend(
    brain: &mut RetailBrain,
    rules: &RuleSet,
    obs: &Observation<'_>,
    budget: &Budget,
    out: &mut Outbox,
) {
    place_finished(brain, rules, obs, out);
    build_next(brain, rules, obs, budget, out);
    // After the structure decision, because `build_next` is the owner that
    // clears the hold; a reserve set before it would be zeroed by it.
    hold_for_a_second_base(brain, rules, obs, budget);
}

/// Place the structures the yard has finished: the click a player makes on
/// the build area after the cameo turns done.
///
/// A finished building waits for a cell, and until it gets one it holds its
/// category — the yard starts nothing else. The attempt counter is what moves
/// the sweep past a cell the gate admits but the receiver refuses.
fn place_finished(
    brain: &mut RetailBrain,
    rules: &RuleSet,
    obs: &Observation<'_>,
    out: &mut Outbox,
) {
    // The ring the wall plan fills, kept clear of foundations: a barracks or a
    // refinery dropped on it is a hole in the wall that nothing can close
    // afterwards, because a wall is an overlay and cannot be built on the cell
    // a building stands on.
    let reserved = wall_ring(rules, obs);
    let mut waiting: Vec<InternedId> = Vec::new();
    for type_id in brain.census.ready.clone() {
        waiting.push(type_id);
        if !out.intents.is_empty() {
            continue;
        }
        let attempt = brain.site_attempts.get(&type_id).copied().unwrap_or(0);
        // A wall segment goes where the wall plan says, not where the outward
        // sweep would put it: the cell is the whole decision, and a sweep that
        // answered it would fill the first gap it found instead of closing the
        // ring around a building worth walling.
        let planned = obs
            .object_type(rules, type_id)
            .filter(|ty| ty.wall)
            .and_then(|_| super::walls::next_segment(brain, rules, obs))
            .map(|segment| {
                // The one line that says whether the ring is where the plan
                // thinks it is: the cell, and how many cells of gap it closes.
                log::info!(
                    "auto play: wall segment at {:?}, {} standing, closes {} cells",
                    segment.cell,
                    segment.standing,
                    segment.fill
                );
                segment.cell
            });
        // A ready building that cannot be placed holds its factory's queue: the
        // next thing that factory makes waits behind it forever. Refusing to
        // place the wall was therefore a worse bug than placing it off the ring
        // — the defense queue stopped, and no pillbox came out again. The plan
        // has first refusal; when it has no segment the sweep answers, and the
        // warning says which happened.
        if planned.is_none() && obs.object_type(rules, type_id).is_some_and(|ty| ty.wall) {
            log::warn!(
                "auto play: wall plan has no segment for {type_id}; the outgoing sweep places it, which may sit off the ring"
            );
        }
        let Some(cell) = planned.or_else(|| {
            Placement::new(obs, rules)
                .reserving(reserved.clone())
                .cell_for(type_id, attempt)
        }) else {
            // The sweep has to move on even when it finds nothing: retrying the
            // same attempt every frame is the placement loop that holds the
            // queue. Once every attempt has failed the type is left to the yard
            // and the queue is allowed to start the next structure.
            let next = (attempt + 1) % PLACEMENT_ATTEMPTS;
            brain.site_attempts.insert(type_id, next);
            if next == 0 {
                brain.stranded_sites.insert(type_id);
                // Skipping the type is not enough: a house that cannot place
                // what its factory built holds the whole category behind it,
                // and no pillbox comes out while a wall waits for a cell.
                // `FactoryClass::AbandonProduction` (`0x004C9FF0`) is the
                // native exit — it settles the held object the way a sale
                // does and drops the factory — and it is the only thing that
                // frees the slot. Emitted once, when the last attempt fails.
                out.intent(Intent::CancelProduction { type_id, all: true });
                log::warn!(
                    "auto play: no cell for {} after {PLACEMENT_ATTEMPTS} tries; abandoning the build",
                    obs.object_type(rules, type_id)
                        .map_or("?", |ty| ty.id.as_str())
                );
            } else {
                log::info!(
                    "auto play: no cell for {} (attempt {attempt})",
                    obs.object_type(rules, type_id)
                        .map_or("?", |ty| ty.id.as_str())
                );
            }
            continue;
        };
        brain.stranded_sites.remove(&type_id);
        // A structure that is worth more than the wall around it does not go
        // where the enemy's guns already reach: the wishlist's "important
        // buildings must not stand at the very front". A wall segment is
        // exempt — it is placed by its own plan, and the ring it closes is
        // drawn around buildings the house already has.
        //
        // The reading is `terrain::Space`, the same vocabulary `expand` uses
        // for a second base, so "the front" means one thing in this brain.
        let is_wall = obs.object_type(rules, type_id).is_some_and(|ty| ty.wall);
        if !is_wall {
            let space = super::terrain::Space::read(
                obs,
                rules,
                &brain.census.home_cells,
                brain.census.base_cell,
            );
            if !space.ground_at(cell).buildable() {
                let next = (attempt + 1) % PLACEMENT_ATTEMPTS;
                brain.site_attempts.insert(type_id, next);
                log::info!(
                    "auto play: {} refused {cell:?}, the enemy's reach is there (attempt {attempt})",
                    obs.object_type(rules, type_id)
                        .map_or("?", |ty| ty.id.as_str())
                );
                continue;
            }
        }
        log::debug!(
            "auto play: place {} at {cell:?} (attempt {attempt})",
            obs.object_type(rules, type_id)
                .map_or("?", |ty| ty.id.as_str())
        );
        out.intent(Intent::PlaceBuilding { type_id, cell });
        let next = (attempt + 1) % PLACEMENT_ATTEMPTS;
        brain.site_attempts.insert(type_id, next);
    }
    brain.site_attempts.retain(|id, _| waiting.contains(id));
}

/// The cells a wall would stand on around the house's own buildings that retail
/// marks `ProtectWithWall=yes`: one ring outside each foundation.
///
/// This is the ring [`super::walls::next_segment`] closes, and the reason a
/// foundation may not be placed on it. Walls only start once the base has
/// [`super::walls::WALL_MIN_STRUCTURES`] structures, so by the time the first
/// segment is queued every cell beside the yard already carries a building
/// unless the sweep was told to leave them alone.
fn wall_ring(rules: &RuleSet, obs: &Observation<'_>) -> BTreeSet<(u16, u16)> {
    let mut ring = BTreeSet::new();
    for entity in obs.own_entities() {
        let Some(ty) = obs.object_type(rules, entity.type_id) else {
            continue;
        };
        if !super::walls::protected(ty) {
            continue;
        }
        let (width, height) = ty.foundation.dimensions();
        // The ring the plan actually builds on, not the one `perimeter`
        // answers: the wall stands `WALL_OFFSET_CELLS` out, and reserving the
        // cell beside the foundation instead left every segment a ring
        // further out than the space kept for it — one cell off, in the same
        // direction on all four sides, which is what "the wall is shifted"
        // looked like.
        ring.extend(super::walls::ring_at(
            entity.cell,
            (width, height),
            super::walls::WALL_OFFSET_CELLS,
        ));
    }
    ring
}

/// The wallet the house keeps for the harvesters it has not built yet.
///
/// Ore delivered is the only income a player's house has, so spending the
/// last of the wallet on something that earns nothing is how a house
/// starves: it ends with a full base and no money to run it. Anything that
/// is not power, income or the factory that makes harvesters waits until the
/// harvesters are paid for.
fn harvester_reserve(rules: &RuleSet, obs: &Observation<'_>, census: &Census) -> i32 {
    if census.harvesters_served() {
        return 0;
    }
    harvester(rules, obs).map_or(0, |(ty, _)| obs.cost_of(rules, ty))
}

/// Whether the type earns its keep: power, income, the factory the harvesters
/// come out of, or the tech the rest of the tree hangs off.
///
/// The tech entries are here for the same reason the refinery is: the oil
/// derrick — the one income a captured building pays — has
/// `[GAOREP] Prerequisite=GATECH,...`, so the lab is income one step removed.
/// Without them the harvester reserve blocked the whole tree for as long as
/// the house was short a harvester, which is how a house reached the end of a
/// match with no lab and no aircraft it could build.
fn earns_its_keep(rules: &RuleSet, ty: &ObjectType) -> bool {
    [
        &rules.build_power_types,
        &rules.build_refinery_types,
        &rules.build_weapons_types,
        &rules.build_radar_types,
        &rules.build_tech_types,
    ]
    .iter()
    .any(|names| names.iter().any(|name| name == &ty.id))
}

/// Start the one structure the base needs most, while the yard is free.
fn build_next(
    brain: &mut RetailBrain,
    rules: &RuleSet,
    obs: &Observation<'_>,
    budget: &Budget,
    out: &mut Outbox,
) {
    // Nothing is being saved for until this frame says what it is: a reserve
    // left over from a decision that has since been paid for would starve the
    // factories of money nobody is saving.
    brain.yard_reserve = 0;
    // A wall segment first, and before the yard's own guard: it is the Defense
    // line's work, so it delays no structure and the plan is not held back by
    // it. The gates inside are the wallet's (a wall is worth a production slot
    // only while the house has money), the line's own, and the base's size.
    if super::walls::queue_segment(brain, rules, obs, budget.free, out) {
        return;
    }
    // A structure the yard has finished and not yet put down is *owned*: it is
    // waiting for a cell, not for a buyer. Starting another because the count
    // on the map has not caught up is how an opening ended with two power
    // plants — the second was queued in the frames the first spent waiting for
    // its cell, and then the yard built both.
    let building = brain.census.line(ProductionCategory::Building);
    let defense = brain.census.line(ProductionCategory::Defense);
    // A stranded type is one the map has no cell for: it still occupies the
    // factory, but it must not hold the whole queue behind it.
    let building_busy = building.product.is_some()
        || building.queued > 0
        || building
            .ready
            .is_some_and(|id| !brain.stranded_sites.contains(&id));
    // The Defense line is busy while it is *working*, not only while it holds
    // a finished piece: a tower in production answered the list every frame
    // for the whole of its build — measured in a real match, 114 pillboxes
    // queued in 110 frames once the midgame wallet stayed above their price.
    let defense_busy = defense.product.is_some()
        || defense.queued > 0
        || defense
            .ready
            .is_some_and(|id| !brain.stranded_sites.contains(&id));
    // The two strips are two factories, and retail builds both at once: the
    // Buildings line and the Defense line have separate queues and separate
    // products. Requiring *both* to be idle before either may be queued was the
    // wishlist's "a pillbox stops the main building" — measured in a real match,
    // the Defense line holding one tower kept the Buildings line from starting
    // anything for the whole of its build.
    if building_busy && defense_busy {
        return;
    }
    let tick = obs.tick();
    let held = brain.plan_hold.filter(|(_, until)| tick < *until);
    let Some(chosen) = next_building(
        rules,
        obs,
        &brain.census,
        budget,
        held.map(|(step, _)| step),
    ) else {
        return;
    };
    // Power is spliced in ahead of the plan and held by nobody, so the entry
    // the house was saving for still stands when the plant is paid for.
    if let Some(step) = chosen.step {
        // The hold is taken when the decision is made, not renewed while it
        // stands: a hold that refreshed every frame would never expire, and
        // the list would answer the same entry for the rest of the match.
        brain.plan_hold = Some((
            step,
            match held {
                Some((held_step, until)) if held_step == step => until,
                _ => tick.saturating_add(BUILD_HOLD_FRAMES),
            },
        ));
    }
    // The build list's answer, with the type it names: the one line that says
    // what the house is building and why, next to the placement that follows
    // it. A plot that was asked for and never stands is visible here.
    // Logged when the answer changes: the list is asked every frame the line
    // is free, and a line per frame is a log nobody reads.
    if chosen.step != held.map(|(step, _)| step) {
        log::info!(
            "auto play: the build list answers {:?} for {}",
            chosen.item,
            chosen.ty.id
        );
    }
    let ty = chosen.ty;
    let type_id = chosen.type_id;
    // Whichever strip owns the answer has to be the free one. The gate above
    // only says the two are not *both* working; this says this one is not.
    // Without it a Defense answer would be queued into a Defense line that is
    // already building, and a Buildings answer into a Buildings line that is.
    if chosen.item == Item::Defense && defense_busy {
        return;
    }
    if chosen.item != Item::Defense && building_busy {
        return;
    }
    // A defense is made by the Defense factory, not the Buildings one, so the
    // guard above does not see it: a tower already on the line is queued again
    // on every frame of its build. Removing the repeats changes the match
    // (measured 2026-10-24: timeout at 36000 instead of a win), because the
    // repeated queue is also what keeps the tower count up, so the two have to
    // be re-balanced together — `DEFENSE_TARGET` at 4 did not make up for it.
    // A structure the yard is already making is not made twice.
    //
    // The busy guard above asks the Building line, and it is not enough: the
    // frame a plant is queued on, the line still reads idle, so the next frame
    // buys the same plant again. That is the opening with two power plants —
    // measured in a real match, both orders logged with `output=0 drain=0
    // power_short=0`, so it was never the power arithmetic: it was the same
    // entry chosen twice. `CanBuild`'s own view of what is already queued is
    // the answer to that, and it does not need the count on the map to catch
    // up.
    //
    // The Defense line is deliberately left out: its repeat is what keeps the
    // tower count up (see below), and that is a separate balance.
    if ty.category == ObjectCategory::Building && obs.in_production(rules, type_id) {
        return;
    }
    let price = obs.cost_of(rules, ty);
    // What the yard has settled on is not the lines' money — but only the tree
    // is worth starving them for. A structure the list has decided to build is
    // paid for as the wallet fills, so the part the house does not have yet is
    // held back from the factories; without it the barracks spent the lab's
    // money on one more soldier every frame and the tech stage never came.
    //
    // Held back for everything, though, it is the barracks that stops: a
    // thousand credits of reserve is thirty seconds of no infantry at all, and
    // a house with no infantry loses the army while it is saving. Only the
    // entries whose whole point is what they unlock are worth that price.
    brain.yard_reserve = match chosen.item {
        Item::Aircraft | Item::Radar | Item::Tech | Item::Superweapon => {
            price.saturating_sub(budget.free).max(0)
        }
        _ => 0,
    };
    // The budget, not the wallet: what a line has been promised is spent
    // whether or not it has left the wallet yet, and a queue costs the wallet
    // nothing until the build steps.
    if budget.free < price {
        return;
    }
    brain.yard_reserve = 0;
    // While the harvesters are still short, only what earns or powers waits
    // for nothing else: a radar bought before the second harvester is the
    // money the second harvester needed.
    if !earns_its_keep(rules, ty) && harvester_reserve(rules, obs, &brain.census) > 0 {
        return;
    }
    // Info, not debug: one line per structure is a handful a match, and it is
    // the only record of *why* the yard started what it did — the opening's
    // two-plant mystery was invisible until these numbers were read.
    log::info!(
        "auto play: build {} at tick {} credits={} power={} fighters={} power_short={} output={} drain={} planned={}",
        ty.id,
        obs.tick(),
        obs.credits(),
        obs.power_balance(),
        brain.census.fighters,
        plan::power_deficit(rules, obs, &brain.census, 0),
        obs.power_output(),
        obs.power_drain(),
        plan::planned_output(rules, obs, &brain.census)
    );
    out.intent(Intent::QueueProduction { type_id });
}

/// Hold the wallet for a second base, when the house has decided it wants one.
///
/// The factories spend everything they are given, so a maker's price is only
/// reachable if something tells the lines to stop first — the same owner that
/// already holds money for the tech stage (`yard_reserve`).
///
/// Measured on Arena.mmx seed 0x5EED with the death ledger on: the wallet
/// peaked at 4550 before any harvester existed and then ran between 400 and
/// 2400 for the whole match, so `harvesters_served` and the maker's price were
/// never true on the same frame — three `second base site` lines and no maker
/// in any roster, for 24k ticks.
///
/// The hold is set from the precondition half of the purchase question rather
/// than all of it: a house that is saving for a maker is exactly a house whose
/// wallet has not reached one, so asking the whole predicate would never
/// reserve anything.
fn hold_for_a_second_base(
    brain: &mut RetailBrain,
    rules: &RuleSet,
    obs: &Observation<'_>,
    budget: &Budget,
) {
    let worth = super::expand::expansion_is_worth_saving_for(brain, rules, obs);
    let needed = super::expand::expansion_price(rules, obs);
    // One line every forty seconds while the house is *not* saving, because
    // "it never buys a maker" has to be answerable from the log the client
    // already writes: which of the preconditions refused, and what the wallet
    // and the price were on that frame.
    if !worth && obs.tick() % 600 == 0 {
        log::info!(
            "auto play: no second base - yard={} maker={} yards={} harv={} ref={} price={} credits={}",
            super::expand::yard_stands(&brain.census),
            super::expand::owns_yard_maker(&brain.census),
            brain.census.yards,
            brain.census.harvesters,
            brain.census.refineries,
            needed,
            budget.free
        );
    }
    if !worth {
        return;
    }
    brain.yard_reserve = needed.saturating_sub(budget.free).max(0);
}

/// The structure to start now.
///
/// Power is spliced in ahead of the choice, the way a player does it: a
/// structure that would take the house past its output is not started until
/// a plant stands. Native does the same splice in `AI_Choose_Building`
/// (`0x004FE6F6..0x004FE943`); here it is the house's own balance and the
/// type's own `Power=`/`ExtraPower=` figure, both read from their owners.
///
/// Each entry asks the rules' own list, so a side that has no such type — or
/// a house that cannot build it yet — simply falls through.
fn next_building<'r>(
    rules: &'r RuleSet,
    obs: &Observation<'_>,
    census: &Census,
    budget: &Budget,
    held: Option<usize>,
) -> Option<plan::Chosen<'r>> {
    // A house with no tower at all builds one before anything else: the gate
    // that holds the army home waits for a tower to stand, and with seven
    // houses attacking the plan's Defense step sat behind a refinery that kept
    // being rebuilt instead — measured, a 1v7 held its army in the yard for
    // the whole match because `base_defended` was false in 24 of 59 samples.
    if !base_defended(rules, obs, census) {
        if let Some((ty, type_id)) = defense_building(rules, obs, census) {
            return Some(plan::Chosen {
                step: None,
                item: Item::Defense,
                ty,
                type_id,
            });
        }
    }
    // A brownout is the one state that stops everything else: production
    // runs at a crawl and the defenses go offline. Whatever else the base
    // wants, a plant comes first, and it comes first even when nothing else
    // is left to build.
    if plan::power_short(rules, obs, census) {
        if let Some((ty, type_id)) = first_buildable(rules, obs, &rules.build_power_types) {
            return Some(plan::Chosen {
                step: None,
                item: Item::Power,
                ty,
                type_id,
            });
        }
    }
    let chosen = match plan::answer(rules, obs, census, budget, held) {
        Some(found) => found,
        // Nothing the base wants is buildable yet, and the house has no plant:
        // `Power=` plants are the prerequisite most structures hang off in the
        // retail rules, so a yard that can only make one is a yard that needs
        // a plant. With a plant already standing there is nothing to add — the
        // house waits for whatever it is missing instead of building blindly.
        None if count_in(obs, rules, census, &rules.build_power_types) == 0 => {
            let (ty, type_id) = first_buildable(rules, obs, &rules.build_power_types)?;
            plan::Chosen {
                step: None,
                item: Item::Power,
                ty,
                type_id,
            }
        }
        None => return None,
    };
    // `Power=` is the type's net contribution: negative is what it draws, and
    // the plant that would feed it is counted at the output it will have. The
    // lab's own `-100` is a plant the base needs before it starts the lab.
    if plan::power_deficit(rules, obs, census, chosen.ty.power.min(0)) > 0 {
        if let Some((ty, type_id)) = first_buildable(rules, obs, &rules.build_power_types) {
            return Some(plan::Chosen {
                step: None,
                item: Item::Power,
                ty,
                type_id,
            });
        }
    }
    Some(chosen)
}

/// Whether the house's own country owns `ty`: `Owner=` is the rules' list of
/// the countries a type belongs to, and it is the country gate the native
/// AI's own build selector reads (`HouseClass__FirstBuildableFromArray`).
///
/// The units carry it — `[E1] Owner=British,French,Germans,Americans,Alliance`
/// is the GI and `[CONA] Owner=Russians,Confederation,Africans,Arabs,YuriCountry`
/// is the conscript — while the structures name every country, because the
/// structure a country builds is the one its own build list names. A brain
/// that picks out of the rules array without this can hand the war factory
/// the other side's tank: the factory refuses it, the order is spent, and
/// the line that was going to make a tank makes nothing.
///
/// An empty list is no evidence and keeps the type available: a mod that
/// names no country is not a mod that forbids the type.
///
/// This is the native AI's own gate, bit for bit: `HouseClass__FirstBuildableFromArray`
/// (`0x005051E0`) tests `Owner=` against the house's *owner* mask (the parent
/// country) and `RequiredHouses=` against the *actual* one, and this is the
/// first half of that test. It is deliberately not in the simulation's
/// `can_build`: native's `HouseClass::CanBuild` (`0x004F7976`) reads only
/// `RequiredHouses=` and `ForbiddenHouses=`, because `Owner=` is the sidebar's
/// and the AI selector's gate. This brain is the AI's selector for its house,
/// so the gate belongs here.
pub(super) fn house_owns(rules: &RuleSet, obs: &Observation<'_>, ty: &ObjectType) -> bool {
    if ty.owner.is_empty() {
        return true;
    }
    let Some(country) = obs.country() else {
        return true;
    };
    let mask = rules.house_type_mask(&ty.owner);
    if mask == 0 {
        // Names the rules do not know are not a refusal.
        return true;
    }
    mask & rules.country_owner_mask(&country) != 0
}

/// The type from `names` the house may build now, in list order.
///
/// `can_build` is the production path's own test, so a type of another side,
/// one whose prerequisite is missing or one beyond the house's tech level
/// answers No here exactly as it would in the sidebar.
pub(super) fn first_buildable<'r>(
    rules: &'r RuleSet,
    obs: &Observation<'_>,
    names: &[String],
) -> Option<(&'r ObjectType, InternedId)> {
    names.iter().find_map(|name| {
        let ty = rules.object(name)?;
        let type_id = obs.type_id(name)?;
        (obs.can_build(rules, ty, true) != CanBuild::No && house_owns(rules, obs, ty))
            .then_some((ty, type_id))
    })
}

/// The house's own harvester: the first `ResourceGatherer=` Vehicle it may
/// build. Ore delivered is the whole income, so this one is not a choice
/// among equals, it is what pays for the rest.
pub(super) fn harvester<'r>(
    rules: &'r RuleSet,
    obs: &Observation<'_>,
) -> Option<(&'r ObjectType, InternedId)> {
    class_types(
        rules,
        obs,
        crate::rules::object_type::ObjectCategory::Vehicle,
    )
    .find(|(ty, _)| ty.resource_gatherer)
}

/// Every type of `category` the rules' array holds, in array order, with its
/// id: the house's side or its prerequisites aside, so a caller can ask what a
/// type it may not build yet would need.
pub(super) fn all_class_types<'r>(
    rules: &'r RuleSet,
    obs: &Observation<'_>,
    category: crate::rules::object_type::ObjectCategory,
) -> impl Iterator<Item = (&'r ObjectType, InternedId)> {
    let count = rules.type_array_ids(category).len();
    (0..count).filter_map(move |index| {
        let ty = rules.type_array_at(category, i32::try_from(index).ok()?)?;
        let type_id = obs.type_id(&ty.id)?;
        Some((ty, type_id))
    })
}

/// Every type of `category` the house may build now, in the rules' array
/// order, with its id.
pub(super) fn class_types<'r>(
    rules: &'r RuleSet,
    obs: &Observation<'_>,
    category: crate::rules::object_type::ObjectCategory,
) -> impl Iterator<Item = (&'r ObjectType, InternedId)> {
    all_class_types(rules, obs, category)
        .filter(|(ty, _)| obs.can_build(rules, ty, true) != CanBuild::No)
        .filter(|(ty, _)| house_owns(rules, obs, ty))
}

/// The `[AI]` base-defense list of the house's own side: what the native
/// computer would answer a raid with, read for the same question here.
pub(super) fn base_defenses<'r>(rules: &'r RuleSet, obs: &Observation<'_>) -> &'r [String] {
    obs.side_index()
        .map_or(&[], |side| rules.base_defense_types(side))
}

/// What a defensive building's own weapons can reach.
///
/// The wishlist's "防御建筑不太会造防空…建议评估防御设施本身的主武器能力". The
/// answer is in the rules, not in a name list: a weapon names a projectile, and
/// the projectile carries `AA=` and `AG=` — the same pair the fire error reads
/// to decide whether a shot is legal (`weapon_class_bits`,
/// `src/sim/combat/combat_weapon.rs`). A mod that gives its tower an
/// anti-aircraft missile is answered without another table here.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct Armed {
    /// At least one firing slot reaches aircraft (`AA=yes`).
    pub(super) anti_air: bool,
    /// At least one firing slot reaches ground (`AG=yes`).
    pub(super) anti_ground: bool,
    /// The longest `Range=` among the firing slots, in cells: how far the
    /// structure covers without moving.
    pub(super) range: i32,
}

/// The `Armed` reading of one building type, from its own weapon slots.
pub(super) fn armament(rules: &RuleSet, ty: &ObjectType) -> Armed {
    let mut armed = Armed::default();
    for slot in 0..ty.weapon_record_count() {
        let Some(weapon) = ty.weapon_at(slot).and_then(|name| rules.weapon(name)) else {
            continue;
        };
        // `NeverUse=` is a scanner, not a weapon: a dog's jaw beside its
        // `NeverUse` scanner is how the scanner is told apart from the bite.
        if weapon.never_use {
            continue;
        }
        // `Range=` is in leptons — `256` per cell, `LEPTONS_PER_CELL_I32` —
        // and the tower's reach in cells is what the aim compares against a
        // Chebyshev distance. Rounded up, so a target in the cell the weapon's
        // edge falls in is still inside.
        armed.range = armed.range.max(
            (weapon.range_leptons.max(0) + crate::util::lepton::LEPTONS_PER_CELL_I32 - 1)
                / crate::util::lepton::LEPTONS_PER_CELL_I32,
        );
        match rules.weapon_projectile(weapon) {
            // A weapon naming no projectile section reads the reader's
            // defaults (AA=no, AG=yes), the same way the fire error does.
            None => armed.anti_ground = true,
            Some(projectile) => {
                armed.anti_air |= projectile.aa;
                armed.anti_ground |= projectile.ag;
            }
        }
    }
    armed
}

/// The defensive building the house should put up now.
///
/// Two answers, in the order the base needs them.
///
/// **The sky, while it is open.** A tower that cannot shoot up does nothing
/// about a bomber, and a base whose towers are all prism towers loses its
/// refinery to the first aircraft that arrives. So the ring is held to a mix:
/// while the house has fewer anti-air towers than one in [`ANTI_AIR_SHARE`] of
/// its ring, the pick is an anti-air one — the one that also shoots ground,
/// when the rules offer one.
///
/// **Otherwise the dearest thing the tech allows.** The `[AI]` list is the
/// native answer and it is empty for some sides (a player's own side in a mod,
/// or a side the list does not name), which left the house with no towers at
/// all. When it is empty, any buildable building whose `Primary=` is a weapon
/// is the answer: that key is the rules' own statement that the structure
/// fires.
///
/// The list's order is not a priority: for the Americans it names the pillbox
/// and the prism tower both, and taking the first buildable one bought a
/// pillbox every time for a whole match — the wishlist's "光棱塔 never appears".
/// A prism tower costs 1500 against a pillbox's 200 and needs `RADAR`, so the
/// dear one is what the tech level is asking for; the pillbox is the answer
/// before the radar stands and the fallback when nothing better is buildable.
///
/// `CanBuild` has already filtered by prerequisite and owner, so "buildable"
/// here means the house has the tree for it: the escalation is the tech, not a
/// guess about it.
pub(super) fn defense_building<'r>(
    rules: &'r RuleSet,
    obs: &Observation<'_>,
    census: &Census,
) -> Option<(&'r ObjectType, InternedId)> {
    let listed = base_defenses(rules, obs);
    let candidates: Vec<(&ObjectType, InternedId)> = if listed.is_empty() {
        class_types(rules, obs, ObjectCategory::Building)
            .filter(|(ty, _)| ty.weapon_at(0).is_some())
            .collect()
    } else {
        listed
            .iter()
            .filter_map(|name| {
                let ty = rules.object(name)?;
                let type_id = obs.type_id(name)?;
                (obs.can_build(rules, ty, true) != CanBuild::No && house_owns(rules, obs, ty))
                    .then_some((ty, type_id))
            })
            .collect()
    };
    if candidates.is_empty() {
        return None;
    }
    let (anti_air_towers, ring) = armed_defenses(rules, obs, census);
    // The sky has to be a real threat before the base pays for AA. `[NASAM]` is
    // 1500 credits and its missile has `AG=no`, so a house that buys one against
    // an enemy with no aircraft has spent the price of a prism tower on nothing
    // — and the wishlist asks for defences chosen by what they can shoot, not by
    // a quota. Once the enemy has shown aircraft, the mix holds.
    let enemy_air = census.threat[crate::rules::unit_role::Role::AntiAir.index()] > 0;
    // Fewer than one anti-air tower per `ANTI_AIR_SHARE` defences: buy one.
    if enemy_air
        && anti_air_towers * ANTI_AIR_SHARE <= ring
        && let Some(found) = candidates
            .iter()
            .filter(|(ty, _)| armament(rules, ty).anti_air)
            // A tower that shoots both ways is worth more than one that only
            // shoots up, and both are worth more than nothing.
            .max_by_key(|(ty, _)| {
                let armed = armament(rules, ty);
                (armed.anti_ground, obs.cost_of(rules, ty))
            })
    {
        return Some(*found);
    }
    candidates
        .into_iter()
        .max_by_key(|(ty, _)| (obs.cost_of(rules, ty), ty.id.clone()))
}

/// How many defences the house has, and how many of them can shoot at aircraft.
fn armed_defenses(rules: &RuleSet, obs: &Observation<'_>, census: &Census) -> (usize, usize) {
    let mut anti_air = 0;
    let mut ring = 0;
    for unit in &census.units {
        let Some(ty) = obs.object_type(rules, unit.type_id) else {
            continue;
        };
        // Only armed structures are defences. The yard's own gun is not one: it
        // is where the house lives, not part of the ring around it.
        if ty.category != ObjectCategory::Building
            || ty.construction_yard
            || ty.weapon_at(0).is_none()
        {
            continue;
        }
        ring += 1;
        if armament(rules, ty).anti_air {
            anti_air += 1;
        }
    }
    (anti_air, ring)
}

/// How many ground defences one anti-air tower is worth.
///
/// One in three: a ring of prism towers with one patriot beside it answers both
/// a tank push and a bombing run, and a ring of patriots answers only the
/// second.
pub(super) const ANTI_AIR_SHARE: usize = 3;

/// How many defensive structures the house has: the `[AI]` list's types, or
/// a building that shoots when the list names none.
pub(super) fn defenses(rules: &RuleSet, obs: &Observation<'_>, census: &Census) -> usize {
    let named = base_defenses(rules, obs);
    if named.is_empty() {
        return census
            .units
            .iter()
            .filter(|unit| {
                obs.object_type(rules, unit.type_id).is_some_and(|ty| {
                    ty.category == ObjectCategory::Building && ty.weapon_at(0).is_some()
                })
            })
            .count();
    }
    count_in(obs, rules, census, named)
}

/// The `UnitRepair=` shed the house can build now (`[GADEPT]`
/// `UnitRepair=yes`, `ini/rulesmd.ini`): the one building a damaged unit can
/// enter, pay at, and leave repaired.
///
/// Read off the key rather than a name list, so a mod's depot is this same
/// type: the retreat already clicks whatever building answers `UnitRepair=`
/// (`census::repair_building`), and the build list has to buy that one.
pub(super) fn unit_repair_building<'r>(
    rules: &'r RuleSet,
    obs: &Observation<'_>,
) -> Option<(&'r ObjectType, InternedId)> {
    class_types(rules, obs, ObjectCategory::Building).find(|(ty, _)| ty.unit_repair)
}

/// The ore purifier: the structure that makes every load worth more.
///
/// `OrePurifier=yes` is the rules' own statement, so a mod that renames the
/// building or adds a second one is answered by the same query — the wishlist's
/// "我们不能写死任何东西".
///
/// It is not wanted early. `[GAOREP] Prerequisite=GATECH,PROC,GACNST` and
/// `Cost=2500`: it is worth what it costs only once there are bays to feed it,
/// which is why the plan asks for it after the second and third refineries and
/// behind a surplus. `BuildLimit=1` in the rules bounds it; the plan's own
/// `want: 1` agrees, so the two cannot disagree.
pub(super) fn ore_purifier_building<'r>(
    rules: &'r RuleSet,
    obs: &Observation<'_>,
) -> Option<(&'r ObjectType, InternedId)> {
    class_types(rules, obs, ObjectCategory::Building)
        .filter(|(ty, _)| ty.ore_purifier)
        .min_by_key(|(ty, _)| (obs.cost_of(rules, ty), ty.id.clone()))
}

/// How many of the house's buildings carry `OrePurifier=`.
pub(super) fn count_ore_purifiers(rules: &RuleSet, obs: &Observation<'_>) -> usize {
    obs.own_entities()
        .filter(|entity| {
            obs.object_type(rules, entity.type_id)
                .is_some_and(|ty| ty.ore_purifier)
        })
        .count()
}

/// The `FactoryPlant=` structure: the industrial plant, `[NAINDP]` in retail.
///
/// `UnitsCostBonus=0.75` is a quarter off every vehicle, and `AIBuildThis=yes`
/// says the computer builds it in retail, so this is the native plan rather than
/// a VERA preference. Read off the flag, not the name.
pub(super) fn industrial_plant_building<'r>(
    rules: &'r RuleSet,
    obs: &Observation<'_>,
) -> Option<(&'r ObjectType, InternedId)> {
    class_types(rules, obs, ObjectCategory::Building)
        .filter(|(ty, _)| ty.factory_plant)
        .min_by_key(|(ty, _)| (obs.cost_of(rules, ty), ty.id.clone()))
}

/// How many `FactoryPlant=` buildings the house has.
pub(super) fn count_industrial_plants(rules: &RuleSet, obs: &Observation<'_>) -> usize {
    obs.own_entities()
        .filter(|entity| {
            obs.object_type(rules, entity.type_id)
                .is_some_and(|ty| ty.factory_plant)
        })
        .count()
}

/// The lab, preferring the one that authorises a power the house can use.
///
/// A super weapon is not granted by the power existing: it is granted by the
/// building that carries it (`SuperWeapon=` / `SuperWeapon2=`,
/// `src/rules/object_type.rs:3610-3611`, read by
/// `grant_provided_supers` in `src/sim/superweapon/mod.rs:1905`), and
/// `[AI] BuildTech=` names more than one lab. Taking the first one is how a
/// house spends a whole match holding a paradrop it was never authorised to
/// fire — which is what "the support powers are never used" turns out to be.
pub(super) fn tech_building<'r>(
    rules: &'r RuleSet,
    obs: &Observation<'_>,
) -> Option<(&'r ObjectType, InternedId)> {
    class_types(rules, obs, ObjectCategory::Building)
        .filter(|(ty, _)| rules.build_tech_types.iter().any(|name| *name == ty.id))
        .min_by_key(|(ty, id)| (u8::from(!grants_support(rules, ty)), *id))
}

/// Whether a building grants a power that changes the map: a missile, a
/// storm, a shield over the army, a warp.
///
/// The split is the same one [grants_support] makes, from the other side.
/// A support power is a look or a delivery and the lab already carries one;
/// a strike is what a superweapon plot is bought for, and the plot is a
/// building like any other -- [GAWEAT] SuperWeapon=WeatherStormSpecial,
/// [GACSPH] SuperWeapon=ChronoSphereSpecial, [NAMISL] NukeSilo=yes -- so the
/// rules answer which building this house may put up, and no name list has
/// to.
pub(super) fn grants_a_strike(rules: &RuleSet, ty: &ObjectType) -> bool {
    ty.nuke_silo
        || [ty.super_weapon.as_deref(), ty.super_weapon2.as_deref()]
            .into_iter()
            .flatten()
            .filter_map(|name| rules.super_weapon(name))
            .any(|power| {
                // The offensive ones. Iron Curtain and Force Shield are left
                // out on purpose: both are a shield over the house's own army,
                // the lab already grants the Force Shield ([GATECH]
                // SuperWeapon=ForceShieldSpecial), and counting the lab as a
                // plot made the step see itself as already answered.
                matches!(
                    power.kind,
                    SuperWeaponKind::MultiMissile
                        | SuperWeaponKind::LightningStorm
                        | SuperWeaponKind::ChronoSphere
                        | SuperWeaponKind::PsychicDominator
                        | SuperWeaponKind::GeneticConverter
                )
            })
}

/// The building that would put a strike in the sidebar, when one may be built
/// now. None before the tree is open, which is the same answer the sidebar
/// gives.
pub(super) fn superweapon_building<'r>(
    rules: &'r RuleSet,
    obs: &Observation<'_>,
) -> Option<(&'r ObjectType, InternedId)> {
    // The one that takes a base apart first. A Chronosphere is a strike by
    // the rules' classification and a teleport by its kind: it moves an army
    // and kills nothing, and it was the plot the house bought first because
    // it comes first in the array. A storm or a missile is what a plot is
    // bought for.
    class_types(rules, obs, ObjectCategory::Building)
        .filter(|(ty, _)| grants_a_strike(rules, ty))
        .min_by_key(|(ty, id)| (u8::from(!damages_a_base(rules, ty)), *id))
}

/// Whether a strike building's power takes hit points off a base rather than
/// moving the house's own army: a storm, a missile, a dominator.
fn damages_a_base(rules: &RuleSet, ty: &ObjectType) -> bool {
    ty.nuke_silo
        || [ty.super_weapon.as_deref(), ty.super_weapon2.as_deref()]
            .into_iter()
            .flatten()
            .filter_map(|name| rules.super_weapon(name))
            .any(|power| {
                matches!(
                    power.kind,
                    SuperWeaponKind::MultiMissile
                        | SuperWeaponKind::LightningStorm
                        | SuperWeaponKind::PsychicDominator
                        | SuperWeaponKind::GeneticConverter
                )
            })
}

/// How many of the house's standing buildings grant a strike, counting what is
/// on the map and not what is queued.
pub(super) fn superweapons(rules: &RuleSet, obs: &Observation<'_>, census: &Census) -> usize {
    census
        .by_type
        .iter()
        .filter(|(type_id, _)| {
            obs.object_type(rules, **type_id)
                .is_some_and(|ty| grants_a_strike(rules, ty))
        })
        .map(|(_, count)| *count)
        .sum()
}

/// Whether a building authorises one of the powers this brain aims at
/// something other than its own force: a paradrop, a spy plane, a reveal.
fn grants_support(rules: &RuleSet, ty: &ObjectType) -> bool {
    [ty.super_weapon.as_deref(), ty.super_weapon2.as_deref()]
        .into_iter()
        .flatten()
        .filter_map(|name| rules.super_weapon(name))
        .any(|power| {
            matches!(
                power.kind,
                SuperWeaponKind::ParaDrop
                    | SuperWeaponKind::AmerParaDrop
                    | SuperWeaponKind::SpyPlane
                    | SuperWeaponKind::PsychicReveal
                    | SuperWeaponKind::UnitDelivery
            )
        })
}

/// Whether the base has anything standing between a raid and its yard.
pub(super) fn base_defended(rules: &RuleSet, obs: &Observation<'_>, census: &Census) -> bool {
    count_in(obs, rules, census, base_defenses(rules, obs)) > 0
}

/// Whether the base can replace what it loses: a war factory for the
/// vehicles, a barracks for the infantry.
///
/// A push spends the army it was given, and a house with no factory to
/// replace it has traded its whole force for whatever it killed. That is why
/// this is the gate on attacking rather than a bigger army alone.
pub(super) fn can_replace_units(rules: &RuleSet, obs: &Observation<'_>, census: &Census) -> bool {
    count_in(obs, rules, census, &rules.build_weapons_types) > 0
        && count_in(obs, rules, census, &rules.build_barracks_types) > 0
}

/// The building a production line needs, when the house has not got one: the
/// rules' own answer, read off `Factory=` rather than off a name list.
///
/// A line can only run where a factory of its category stands — `Aircraft` is
/// `Factory=AircraftType`, and the retail rules hold exactly two such
/// buildings, `GAAIRC` and `AMRADR`, both Allied: `[GAAIRC]
/// ForbiddenHouses=Americans` and `[AMRADR] RequiredHouses=Americans` split them
/// between the allied countries, and the Soviets have none at all. That is a
/// fact about the rules, not a gap in this brain, so the answer here is the
/// same `CanBuild` the sidebar asks.
///
/// `None` when the house already owns a factory of the category — the line
/// runs and there is nothing to build — or when nothing of the category is
/// buildable for it.
pub(super) fn line_factory<'r>(
    rules: &'r RuleSet,
    obs: &Observation<'_>,
    census: &Census,
    category: ProductionCategory,
) -> Option<(&'r ObjectType, InternedId)> {
    let factory = match category {
        ProductionCategory::Aircraft => FactoryType::AircraftType,
        ProductionCategory::Building | ProductionCategory::Defense => {
            return None;
        }
        // The infantry and vehicle plants are named by `[AI] Build*=` and are
        // already steps of this order; a naval yard needs water this brain does
        // not look for, so it is not asked for.
        ProductionCategory::Infantry | ProductionCategory::Vehicle | ProductionCategory::Ship => {
            return None;
        }
    };
    let candidates: Vec<(&ObjectType, InternedId)> =
        class_types(rules, obs, ObjectCategory::Building)
            .filter(|(ty, _)| ty.factory_type() == Some(factory))
            .collect();
    if candidates
        .iter()
        .any(|(_, type_id)| census.type_count(*type_id) > 0)
    {
        return None;
    }
    candidates.into_iter().next()
}

/// How many of the house's objects are one of `names`.
pub(super) fn count_in(
    obs: &Observation<'_>,
    _rules: &RuleSet,
    census: &Census,
    names: &[String],
) -> usize {
    census.count_named(obs, names)
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::sim::world::Simulation;

    /// The retail rival arena, needed to field real object types.
    ///
    /// The rules come from the repository's own `ini/` snapshot rather than a
    /// retail install, so this order — the thing a play test measures — is
    /// regression tested wherever the sources are checked out.
    fn arena(seed: u64) -> Option<(Simulation, RuleSet)> {
        let rules = crate::rules::retail_ini_fixture::retail_unlayered_rules()?;
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, seed);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        Some((sim, rules))
    }

    /// The ledger the order is decided from, built the way `think` builds it.
    fn ledger(rules: &RuleSet, sim: &Simulation, owner: InternedId) -> Census {
        let mut census = Census::default();
        census.refresh(
            &crate::rules::unit_role::RoleTable::build(rules),
            rules,
            &Observation::new(sim, owner),
        );
        census
    }

    /// The opening is the plant, then the barracks, then the income, then the
    /// tech: power is what every other structure's `Prerequisite=` asks for, the
    /// barracks is the cheaper production building and the one the engineer and
    /// the dog come out of, and the tree is unlocked before a second line or a
    /// second tower is bought.
    /// The industrial plant is selected by `FactoryPlant=yes`, not by name, and
    /// only for a house whose tree can reach it: `Prerequisite=NATECH,PROC` is
    // the Soviet tree, so an American house has no answer and is not offered
    // one.
    #[test]
    fn the_industrial_plant_is_chosen_by_its_factory_flag() {
        let Some((mut sim, rules)) = arena(0xA17) else {
            return;
        };
        let soviet = sim.interner.intern("Russians");
        // Spread out: each foundation has to fit, and a yard's is 3x3.
        for (name, cell) in [
            ("NACNST", (16u16, 16u16)),
            ("NAREFN", (24, 16)),
            ("NATECH", (16, 24)),
        ] {
            sim.spawn_object(name, "Russians", cell.0, cell.1, 255, &rules)
                .unwrap_or_else(|| panic!("place {name}"));
        }
        let obs = Observation::new(&sim, soviet);
        let (ty, _) = industrial_plant_building(&rules, &obs).expect("the plant is buildable");
        assert_eq!(ty.id.as_str(), "NAINDP", "`FactoryPlant=yes` names it");
        assert!(ty.factory_plant, "and the flag is what selected it");

        let american = sim.interner.intern("Americans");
        let obs = Observation::new(&sim, american);
        assert!(
            industrial_plant_building(&rules, &obs).is_none(),
            "the Allied house cannot build the Soviet plant"
        );
    }

    #[test]
    fn a_towers_armament_comes_from_its_own_weapons() {
        let Some((_sim, rules)) = arena(0xA17) else {
            return;
        };
        // The prism tower's `Primary=PrismShot` names `InvisibleHigh`, whose
        // `AA=` is no: it is a ground weapon, and the reading has to say so.
        let prism = rules.object("ATESLA").expect("the prism tower");
        let prism = armament(&rules, prism);
        assert!(prism.anti_ground, "the prism tower shoots ground");
        assert!(!prism.anti_air, "and it does not shoot aircraft");
        assert_eq!(prism.range, 8, "`Range=8` is eight cells");

        // The patriot's `Primary=RedEye2` names `AAHeatSeeker`, whose `AA=` is
        // yes: the anti-air tower the mix asks for.
        let patriot = rules.object("NASAM").expect("the patriot");
        let patriot = armament(&rules, patriot);
        assert!(patriot.anti_air, "the patriot shoots aircraft");
        assert!(!patriot.anti_ground, "and `AAHeatSeeker` is air-only");
        assert_eq!(patriot.range, 12, "`Range=12` is twelve cells");
    }

    #[test]
    fn the_opening_is_power_then_barracks_then_income_then_tech() {
        let Some((mut sim, rules)) = arena(0xA17) else {
            return;
        };
        let owner = sim.interner.intern("Americans");
        sim.spawn_object("GACNST", "Americans", 16, 16, 255, &rules)
            .expect("an Allied Construction Yard");
        // `GAREFN` and `GAPILE` both want `Prerequisite=POWER,GACNST`
        // (ini/rulesmd.ini:11731, :11692), so the plant is the first thing the
        // list can answer at all.
        let ty = {
            let census = ledger(&rules, &sim, owner);
            let obs = Observation::new(&sim, owner);
            let budget = plan::budget(&rules, &obs, &census, None);
            plan::answer(&rules, &obs, &census, &budget, None)
                .expect("a base with nothing but a yard wants something")
                .ty
        };
        assert!(
            rules.build_power_types.iter().any(|name| name == &ty.id),
            "the first structure is the plant: {}",
            ty.id
        );

        sim.spawn_object("GAPOWR", "Americans", 20, 16, 255, &rules)
            .expect("a power plant");
        sim.houses
            .get_mut(&owner)
            .expect("the house")
            .economy_mut()
            .set_credits_for_test(5_000);

        let census = ledger(&rules, &sim, owner);
        let obs = Observation::new(&sim, owner);
        let budget = plan::budget(&rules, &obs, &census, None);
        let ty = plan::answer(&rules, &obs, &census, &budget, None)
            .expect("a base with power wants the next thing")
            .ty;
        assert!(
            rules.build_barracks_types.iter().any(|name| name == &ty.id),
            "the first production building is the barracks: {}",
            ty.id
        );

        // The barracks on the map: the income, because the production building
        // now stands there.
        sim.spawn_object("GAPILE", "Americans", 24, 24, 255, &rules)
            .expect("a barracks");
        let census = ledger(&rules, &sim, owner);
        let obs = Observation::new(&sim, owner);
        let budget = plan::budget(&rules, &obs, &census, None);
        let ty = plan::answer(&rules, &obs, &census, &budget, None)
            .expect("a base with soldiers wants the next thing")
            .ty;
        assert!(
            rules.build_refinery_types.iter().any(|name| name == &ty.id),
            "the bay comes before the war factory: {}",
            ty.id
        );

        // With the opening in place and money in the wallet, the list answers
        // the tech chain and not a tower: the war factory first, because the
        // lab's own `Prerequisite=GAWEAP,RADAR,GACNST`, and then the aircraft
        // plant that stands in for the radar.
        sim.spawn_object("GAREFN", "Americans", 20, 20, 255, &rules)
            .expect("a refinery");
        let census = ledger(&rules, &sim, owner);
        let obs = Observation::new(&sim, owner);
        let budget = plan::budget(&rules, &obs, &census, None);
        let chosen = plan::answer(&rules, &obs, &census, &budget, None)
            .expect("an open tree is what the house buys next");
        assert_eq!(
            chosen.item,
            plan::Item::Weapons,
            "the war factory is the lab's own prerequisite"
        );
        assert!(
            !base_defenses(&rules, &obs)
                .iter()
                .any(|name| name == &chosen.ty.id),
            "not a tower: {}",
            chosen.ty.id
        );

        sim.spawn_object("GAWEAP", "Americans", 28, 24, 255, &rules)
            .expect("a war factory");
        let census = ledger(&rules, &sim, owner);
        let obs = Observation::new(&sim, owner);
        let budget = plan::budget(&rules, &obs, &census, None);
        let chosen = plan::answer(&rules, &obs, &census, &budget, None)
            .expect("the aircraft plant is what the tree asks for next");
        assert_eq!(chosen.item, plan::Item::Aircraft, "the tech stage");
        assert_eq!(chosen.ty.id, "AMRADR", "the Americans' aircraft plant");
    }

    /// A structure the yard has finished and not yet put down is a structure the
    /// house owns. Counting the map was how an opening bought two power plants:
    /// the second was queued in the frames the first spent waiting for its cell.
    #[test]
    fn a_structure_waiting_for_a_cell_is_not_bought_again() {
        let Some((mut sim, rules)) = arena(0xA17) else {
            return;
        };
        let owner = sim.interner.intern("Americans");
        sim.spawn_object("GACNST", "Americans", 16, 16, 255, &rules)
            .expect("an Allied Construction Yard");
        sim.houses
            .get_mut(&owner)
            .expect("the house")
            .economy_mut()
            .set_credits_for_test(5_000);

        let mut brain = RetailBrain::new(0xA17);
        brain.census = ledger(&rules, &sim, owner);
        // The plant is built and standing in the yard, waiting for its cell.
        let plant = sim.interner().get("GAPOWR").expect("the power plant type");
        brain.census.lines[crate::sim::strategy::retail::census::LINE_BUILDING].ready = Some(plant);
        let obs = Observation::new(&sim, owner);
        let budget = plan::budget(&rules, &obs, &brain.census, None);
        let mut out = Outbox::default();
        spend(&mut brain, &rules, &obs, &budget, &mut out);

        assert!(
            !out.intents
                .iter()
                .any(|intent| matches!(intent, Intent::QueueProduction { .. })),
            "the yard is holding a plant, not asking for a second: {:?}",
            out.intents
        );
    }

    /// The tech buildings are not what the harvester reserve is holding money
    /// back for.
    ///
    /// `earns_its_keep` knew only about power, income and the war factory, so a
    /// house short of a harvester refused to buy the lab — and a house that
    /// never buys the lab never gets the rest of the tree, which is how a match
    /// ended with no lab and no aircraft either. The lab is income one step
    /// removed: `[GAOREP] Prerequisite=GATECH,PROC,GACNST` is the oil derrick.
    #[test]
    fn the_tech_buildings_are_not_what_the_harvester_reserve_holds_money_for() {
        let Some((mut sim, rules)) = arena(0xA17) else {
            return;
        };
        let owner = sim.interner.intern("Americans");
        let obs = Observation::new(&sim, owner);
        for name in ["GATECH", "AMRADR"] {
            let ty = obs
                .type_id(name)
                .and_then(|type_id| obs.object_type(&rules, type_id))
                .unwrap_or_else(|| panic!("{name} is in the rules"));
            assert!(
                earns_its_keep(&rules, ty),
                "{name} is not paid for out of the harvester reserve"
            );
        }
    }

    /// The aircraft plant comes from the rules, not from a name list: the only
    /// two buildings with `Factory=AircraftType` in the retail rules are
    /// `GAAIRC` and `AMRADR`, and which one a house may build is `CanBuild`'s
    /// answer. A side with neither — the Soviets, whose "air headquarters" is
    /// the `NARADR` radar tower with no production — gets no aircraft, because
    /// the rules give it none.
    #[test]
    fn the_aircraft_plant_is_the_rules_aircraft_factory() {
        let Some((mut sim, rules)) = arena(0xA17) else {
            return;
        };
        let owner = sim.interner.intern("Americans");
        for (name, cell) in [
            ("GACNST", (8u16, 8u16)),
            ("GAPOWR", (12, 8)),
            // `[AMRADR] Prerequisite=GAREFN,GACNST`.
            ("GAREFN", (16, 8)),
        ] {
            sim.spawn_object(name, "Americans", cell.0, cell.1, 255, &rules)
                .unwrap_or_else(|| panic!("{name}"));
        }
        let census = ledger(&rules, &sim, owner);
        let obs = Observation::new(&sim, owner);
        let (plant, _) = line_factory(&rules, &obs, &census, ProductionCategory::Aircraft)
            .expect("the Americans have an aircraft plant");
        assert_eq!(plant.id, "AMRADR");
        assert_eq!(plant.factory_type(), Some(FactoryType::AircraftType));

        // Once it stands there is nothing more to build for that line.
        sim.spawn_object("AMRADR", "Americans", (16u16, 16u16).0, 16, 255, &rules)
            .expect("the plant");
        let census = ledger(&rules, &sim, owner);
        assert!(
            line_factory(
                &rules,
                &Observation::new(&sim, owner),
                &census,
                ProductionCategory::Aircraft
            )
            .is_none(),
            "the line's plant already stands"
        );
    }

    /// Two refineries are the cap. A third bay is the dearest structure in the
    /// game and pays only once the harvesters to fill it exist, so a house that
    /// keeps buying them never gets an army.
    #[test]
    fn the_third_refinery_is_off_the_table() {
        let Some((mut sim, rules)) = arena(0xA17) else {
            return;
        };
        let owner = sim.interner.intern("Americans");
        sim.spawn_object("GACNST", "Americans", 16, 16, 255, &rules)
            .expect("a yard");
        sim.spawn_object("GAPOWR", "Americans", 12, 16, 255, &rules)
            .expect("a power plant");
        for cell in [(20u16, 20u16), (24, 24)] {
            sim.spawn_object("GAREFN", "Americans", cell.0, cell.1, 255, &rules)
                .expect("a refinery");
        }
        let census = ledger(&rules, &sim, owner);
        assert_eq!(census.refineries, 2, "two bays stand");
        let obs = Observation::new(&sim, owner);
        let budget = plan::budget(&rules, &obs, &census, None);
        assert!(
            plan::answer(&rules, &obs, &census, &budget, None).is_none_or(|chosen| !rules
                .build_refinery_types
                .iter()
                .any(|name| name == &chosen.ty.id)),
            "a third refinery is not what the base builds next"
        );
    }

    /// One war factory is the target; the second is a surplus decision. A base
    /// that buys two lines before it has an army has bought throughput nothing
    /// is spending.
    #[test]
    fn the_second_war_factory_waits_for_a_surplus() {
        let Some((mut sim, rules)) = arena(0xA17) else {
            return;
        };
        let owner = sim.interner.intern("Americans");
        sim.spawn_object("GACNST", "Americans", 16, 16, 255, &rules)
            .expect("a yard");
        sim.spawn_object("GAPOWR", "Americans", 12, 16, 255, &rules)
            .expect("a power plant");
        sim.spawn_object("GAREFN", "Americans", 20, 20, 255, &rules)
            .expect("a refinery");
        sim.spawn_object("GAPILE", "Americans", 24, 24, 255, &rules)
            .expect("a barracks");
        sim.spawn_object("GAWEAP", "Americans", 28, 28, 255, &rules)
            .expect("a war factory");
        sim.houses
            .get_mut(&owner)
            .expect("the house")
            .economy_mut()
            .set_credits_for_test(2_000);

        let census = ledger(&rules, &sim, owner);
        let obs = Observation::new(&sim, owner);
        let budget = plan::budget(&rules, &obs, &census, None);
        assert!(
            plan::answer(&rules, &obs, &census, &budget, None).is_none_or(|chosen| !rules
                .build_weapons_types
                .iter()
                .any(|name| name == &chosen.ty.id)),
            "a poor house with one war factory does not start another"
        );
    }

    /// A plant that is still building supplies its `Power=` scaled by the
    /// health it has built, so the balance reads a brownout for as long as the
    /// plant is being built. The base used to answer that with a second plant:
    /// the opening became two plants and one refinery, which is a base that
    /// paid for power it did not need.
    #[test]
    fn a_plant_that_is_building_is_the_plant_the_base_was_short_of() {
        let Some((mut sim, rules)) = arena(0xA17) else {
            return;
        };
        let owner = sim.interner.intern("Americans");
        sim.spawn_object("GACNST", "Americans", 16, 16, 255, &rules)
            .expect("a yard");
        sim.spawn_object("GAREFN", "Americans", 20, 20, 255, &rules)
            .expect("a refinery");

        let mut census = ledger(&rules, &sim, owner);
        let plant = sim.interner().get("GAPOWR").expect("the plant's id");
        census.lines[crate::sim::strategy::retail::census::LINE_BUILDING].product = Some(plant);
        let obs = Observation::new(&sim, owner);
        assert!(
            obs.power_balance() < 0,
            "the plant on the line supplies nothing yet: {}",
            obs.power_balance()
        );
        assert!(
            !plan::power_short(&rules, &obs, &census),
            "the plant it is building is the plant it was short of"
        );
    }

    /// A structure the enemy destroyed is a structure the base buys again: the
    /// list counts what *stands*, so a nuclear missile that takes the plant
    /// leaves a house that answers `Power` on the next frame. This is the
    /// whole of "it does not rebuild after a strike" — no plan step is
    /// remembered past the object's own life.
    #[test]
    fn a_plant_that_was_destroyed_is_the_plant_the_base_buys_again() {
        let Some((mut sim, rules)) = arena(0xB01) else {
            return;
        };
        let owner = sim.interner.intern("Americans");
        sim.spawn_object("GACNST", "Americans", 16, 16, 255, &rules)
            .expect("a yard");
        let census = ledger(&rules, &sim, owner);
        let obs = Observation::new(&sim, owner);
        let budget = plan::budget(&rules, &obs, &census, None);
        let chosen = next_building(&rules, &obs, &census, &budget, None)
            .expect("a base with no plant still has something to build");
        assert_eq!(
            chosen.item,
            plan::Item::Power,
            "the plant the base never had is the plant it buys"
        );

        // The same base with its plant standing: the step is answered.
        sim.spawn_object("GAPOWR", "Americans", 24, 20, 255, &rules)
            .expect("a plant");
        let census = ledger(&rules, &sim, owner);
        let obs = Observation::new(&sim, owner);
        let budget = plan::budget(&rules, &obs, &census, None);
        let chosen = next_building(&rules, &obs, &census, &budget, None);
        assert!(
            chosen.is_none_or(|chosen| chosen.item != plan::Item::Power),
            "a plant that stands is not bought twice"
        );
    }

    /// The country gate is the rules': the GI's Owner list names the Allied
    /// countries and the conscript's names the Soviet ones, so an American
    /// house does not own a conscript and a Russian house does not own a GI.
    #[test]
    fn a_house_does_not_own_the_other_side_s_units() {
        let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules() else {
            return;
        };
        let mut sim = crate::sim::world::Simulation::new();
        let american = sim.interner.intern("Americans");
        let russian = sim.interner.intern("Russians");
        sim.houses
            .register(crate::sim::house_state::HouseState::new(
                american,
                0,
                Some(american),
                true,
                0,
                10,
            ));
        sim.houses
            .register(crate::sim::house_state::HouseState::new(
                russian,
                0,
                Some(russian),
                false,
                0,
                10,
            ));
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let gi = rules.object("E1").expect("the GI");
        let cona = rules.object("CONA").expect("the conscript");

        let obs = Observation::new(&sim, american);
        assert_eq!(
            obs.country().as_deref(),
            Some("Americans"),
            "the house has a country"
        );
        assert!(
            house_owns(&rules, &obs, gi),
            "the GI is the American house's own"
        );
        assert!(!house_owns(&rules, &obs, cona), "and the conscript is not");

        let obs = Observation::new(&sim, russian);
        assert!(
            house_owns(&rules, &obs, cona),
            "the conscript is the Russian house's own"
        );
        assert!(!house_owns(&rules, &obs, gi), "and the GI is not");
    }
    /// The building the plot step buys is the one the rules say grants a
    /// strike, not a name list of ours: `[GAWEAT] SuperWeapon=
    /// LightningStormSpecial` and `[NAMISL] SuperWeapon=NukeSpecial` are the
    /// same answer through different keys, and a power plant grants nothing.
    #[test]
    fn a_superweapon_plot_grants_a_strike_and_a_power_plant_does_not() {
        let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules() else {
            return;
        };
        let weather = rules.object("GAWEAT").expect("the allied plot");
        assert!(
            grants_a_strike(&rules, weather),
            "the weather controller is a strike"
        );
        let plant = rules.object("GAPOWR").expect("the plant");
        assert!(!grants_a_strike(&rules, plant), "a power plant is not");
        let lab = rules.object("GATECH").expect("the lab");
        assert!(!grants_a_strike(&rules, lab), "and neither is the lab");
    }
}
