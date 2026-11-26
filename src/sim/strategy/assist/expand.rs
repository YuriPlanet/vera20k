//! Getting a base started: deploy the yard maker, and buy another one when the
//! yard is gone.
//!
//! `UnitClass::Deploy` (`0x007393C0`) is the AI's own route to a base — the
//! house's MCV becomes a Construction Yard, and the Recalc that follows anchors
//! the plan on it. A brain-driven house receives no player click, so the brain
//! issues the same deploy the click would.
//!
//! A yard is also a structure like any other, so it can be destroyed, and every
//! structure in `[AI] BuildRefinery=`/`BuildWeapons=`/`BuildBarracks=` hangs off
//! it through `Prerequisite=` (`HouseClass::CanBuild` @ `0x004F7870`: a human
//! house needs the entry on the map). Losing it is not a lost base yet — the
//! factories still produce and the army still fights — but nothing new can be
//! started, so a player buys another MCV out of the war factory, which is the
//! only type that carries `DeploysInto=` back to the yard.
//!
//! Beyond that first yard, a house that is earning more than it can spend
//! builds another: it buys a maker out of the war factory, walks it away from
//! the base it has, and deploys it there. That is a second `BuildConst=`
//! structure, and the placement sweep, the build list and the walls all read
//! it as one — there is no second ledger for it.

use crate::rules::object_type::{ObjectCategory, ObjectType};
use crate::rules::prerequisite::Prerequisite;
use crate::rules::ruleset::RuleSet;
use crate::sim::control::Goal;
use crate::sim::control::brain::Outbox;
use crate::sim::control::observation::Observation;
use crate::sim::control::placement::{PLACEMENT_ATTEMPTS, Placement};
use crate::sim::intern::InternedId;
use crate::sim::production::CanBuild;
use crate::sim::strategy::assist::AssistBrain;

use super::advance::cell_distance;
use super::census::Census;
use super::plan::Budget;

/// The free wallet a second base is bought out of: the maker's own price and
/// the first structure that follows it out of the new yard.
pub(super) const EXPAND_MIN_CREDITS: i32 = 4_000;

/// The first structure a new yard is expected to buy, held back with the maker.
///
/// A yard with no power plant beside it is a building that produces nothing, so
/// the maker's price alone is not the cost of a second base. It is a *small*
/// hold, though: the plant can be paid for out of the next few hundred credits
/// the house earns, and holding the full price of one alongside the maker is
/// what kept the wallet short of both.
const EXPAND_FIRST_STRUCTURE: i32 = 200;

/// How far from the base it already has the second one is built, in cells: near
/// enough to walk to and to defend, far enough that the two bases do not become
/// one crowded block — which is the shape the placement sweep is built to
/// avoid.
const EXPAND_MIN_CELLS: i32 = 10;

/// How close to the base a retrieved maker has to be before it counts as home.
///
/// `rally_near` answers on rings 2..4, so six cells is the whole answer plus a
/// cell of slack: a maker inside it has arrived, not "arriving forever".
const RETRIEVE_ARRIVED_CELLS: i32 = 6;

/// How long a yard stays committed to the stance the house gave it, in frames.
///
/// [`deploy_yard_maker`] and [`super::orders::pack_captured_yard`] are two halves
/// of one decision about the same object, and neither used to carry commitment:
/// a captured yard that packed home became a maker, the maker read
/// `deploy_site_admits` and was deployed straight back out, and the yard it made
/// was then far from home again — the wishlist's "部署收起部署收起". Sixty
/// seconds is a turn of the expansion probe, long enough for the stance to have
/// meant something and short enough that a changed front still gets a new one.
pub(super) const YARD_COMMIT_FRAMES: u64 = 900;

/// How far out that search looks, in cells.
///
/// A second base goes to the next patch of ground that takes a yard, which
/// is a long way off when the house stands on an ore field: the first yard's
/// own ground is all ore, and the ground that is not ore is the other field.
const EXPAND_SCAN_CELLS: i32 = 48;

/// How far from an ore cell a yard may stand and still be the base that serves
/// it, in cells.
///
/// A harvester's round trip is the ore, the refinery and back; a yard at the
/// edge of this ring is a base whose trucks cross the whole field to unload.
/// Twelve is past the ring the first base already covers
/// ([`EXPAND_MIN_CELLS`] is ten) and short of a trip that eats the advantage.
const EXPAND_ORE_RING: i32 = 12;

/// How long a probe's answer is kept, in frames.
///
/// The search asks the deploy's question cell by cell over the map, so it is
/// a probe rather than a per-frame question: four seconds is a maker's own
/// pace, and a cell that has become unusable is re-asked on the next probe.
const EXPAND_PROBE_FRAMES: u64 = 60;
/// Hold a chosen mineral-side MCV deployment plot through temporary blockers.
const EXPANSION_SITE_COMMIT_FRAMES: u64 = 450;
/// Give friendly infantry time to clear the native yard foundation before
/// abandoning the chosen ore-side expansion plot.
const MCV_CLEARANCE_FRAMES: u64 = 240;
const MCV_REJECT_SITE_FRAMES: u64 = 1800;

/// Whether `ty` is a Construction Yard maker: its `DeploysInto=` is the
/// building the rules' `[AI] BuildConst=` list holds. `UnitType`'s
/// `DeploysInto=` is the reader's own field, so a side whose maker is called
/// something else answers here without a name of its own.
pub(super) fn makes_a_yard(rules: &RuleSet, ty: &ObjectType) -> bool {
    ty.deploys_into
        .as_deref()
        .and_then(|name| rules.object(name))
        .is_some_and(|yard| yard.build_const_eligible())
}

/// Whether a yard stands: `BuildConst=` eligible is the rules' own mark for
/// it, and the ledger already counted them.
pub(super) fn yard_stands(census: &Census) -> bool {
    census.yards > 0
}

/// Whether the house holds a mobile yard maker: an MCV waiting to deploy, or
/// one still driving out of its factory.
pub(super) fn owns_yard_maker(census: &Census) -> bool {
    census.yard_makers > 0
}

/// Whether this yard is still inside the term of a stance already given, so the
/// caller leaves it alone rather than reversing it. Reading and writing
/// [`AssistBrain::yard_commit`] here is the whole commitment: a pack and a deploy
/// are the same decision about the same object, so both halves ask this one
/// question instead of each carrying its own memory of it.
pub(super) fn yard_committed(brain: &AssistBrain, tick: u64, entity_id: u64) -> bool {
    brain
        .yard_commit
        .get(&entity_id)
        .is_some_and(|at| tick.saturating_sub(*at) < YARD_COMMIT_FRAMES)
}

/// Deploy the house's Construction Yard maker while it has no yard.
///
/// A maker that just left the war factory stands where the yard cannot: the
/// deploy shifts its foundation one cell north-west
/// (`deploy_origin_from_unit_cell`), and the factory and its neighbours are
/// in the way. `UnitClass::Deploy` refuses that silently and drops the unit
/// back onto Guard, so the brain does what the player does — drives it to a
/// cell the yard fits on first and presses D there. Native's own answer to a
/// blocked cell is the same search, in `UnitClass::TryToDeploy`
/// (`0x00738D30`), which only a computer house's Hunt mission runs.
pub(super) fn deploy_yard_maker(
    brain: &mut AssistBrain,
    rules: &RuleSet,
    obs: &Observation<'_>,
    out: &mut Outbox,
) {
    // One line every forty seconds (600 frames at 15 a second), because this
    // branch is silent by design: a report of "the maker never deploys" has to
    // be able to say whether the brain ever saw one, and whether a yard was
    // standing over it.
    if obs.tick() % 600 == 0 {
        let maker = brain
            .census
            .units
            .iter()
            .find(|unit| unit.traits.yard_maker);
        // The two numbers that answer "why is it still standing there": how
        // far the maker is from home (the sweep's own minimum), and whether
        // the ground under it would take the deploy. A maker that walks and
        // never lands is one of the two, and neither is visible from here
        // without printing it.
        let far = maker.map_or(0, |unit| {
            brain
                .census
                .base_cell
                .map_or(0, |home| cell_distance(unit.cell, home))
        });
        let admits = maker.is_some_and(|unit| {
            obs.object_type(rules, unit.type_id)
                .and_then(|ty| ty.deploys_into.as_deref())
                .and_then(|name| rules.object(name))
                .is_some_and(|yard| obs.deploy_site_admits(rules, unit.entity_id, yard))
        });
        log::info!(
            "auto play: yard={} maker={:?} far={far} admits={admits} credits={}",
            yard_stands(&brain.census),
            maker.map(|unit| (unit.entity_id, unit.cell)),
            obs.credits()
        );
    }
    // Every maker gets the first-sighting test, not only the one the selection
    // below happens to pick. Two captured MCVs were marked one at a time, so
    // the second was never recalled at all and sat in the enemy's base until
    // it was destroyed — the wishlist's "占领了俩基地只拉回一个".
    //
    // A maker that is not orderable is not on the map: a fresh one sits in the
    // factory's limbo at `(0, 0)` until PLACE puts it down, and counting that
    // as a sighting recalled every MCV the house built. It then tried to walk
    // home from `(0, 0)` — a cell it is not on — and the second base was never
    // founded. That is the "新 MCV 四处打转不找地方部署".
    for unit in &brain.census.units {
        if !unit.traits.yard_maker || unit.hand || !unit.orderable {
            continue;
        }
        let at_home = brain
            .census
            .base_cell
            .is_some_and(|home| cell_distance(unit.cell, home) <= RETRIEVE_ARRIVED_CELLS);
        if brain.seen_makers.insert(unit.entity_id) && !at_home {
            brain.retrieve_makers.insert(unit.entity_id);
            log::info!(
                "auto play: maker {} first seen at {:?}, recalling it",
                unit.entity_id,
                unit.cell
            );
        }
    }
    // A foreign-side maker is a strategic tech seed, not an expendable
    // expansion vehicle. It must come home even when a crate produced it on
    // safe neutral ground, and must not be sent out again after arriving.
    // Own-side MCVs keep their separate second-base behavior.
    if brain.census.base_cell.is_some() {
        for unit in &brain.census.units {
            if !unit.traits.yard_maker || unit.hand || !unit.orderable { continue; }
            let Some(ty) = obs.object_type(rules, unit.type_id) else { continue; };
            let Some(yard) = ty.deploys_into.as_deref().and_then(|name| rules.object(name))
            else { continue; };
            if super::build::foreign_yard(rules, obs, yard) {
                // Reaching home is a one-way transition. The chosen native
                // foundation may be 7-18 cells away from the yard's centre;
                // that is NOT a reason to send this MCV home again.
                if brain.foreign_ready_makers.contains(&unit.entity_id) {
                    brain.retrieve_makers.remove(&unit.entity_id);
                } else if brain.census.base_cell.is_some_and(|home|
                    cell_distance(home, unit.cell) <= RETRIEVE_ARRIVED_CELLS)
                {
                    brain.foreign_ready_makers.insert(unit.entity_id);
                    brain.retrieve_makers.remove(&unit.entity_id);
                } else {
                    brain.retrieve_makers.insert(unit.entity_id);
                }
            }
        }
    }
    // Prefer a maker under recall: with two of them the one already heading
    // home must not be re-aimed while the other is still sitting there.
    let makers: Vec<(u64, (u16, u16), &ObjectType)> = brain
        .census
        .units
        .iter()
        .filter(|unit| unit.traits.yard_maker && !unit.hand && unit.orderable)
        .filter_map(|unit| {
            let ty = obs.object_type(rules, unit.type_id)?;
            let yard = rules.object(ty.deploys_into.as_deref()?)?;
            makes_a_yard(rules, ty).then_some((unit.entity_id, unit.cell, yard))
        })
        .collect();
    brain.maker_journeys.retain(|id, _| makers.iter().any(|maker| maker.0 == *id));
    brain.foreign_ready_makers.retain(|id| makers.iter().any(|maker| maker.0 == *id));
    let picked = makers
        .iter()
        // Finish an already homed technology seed before recalling another
        // MCV, otherwise a blocked distant retrieval can starve deployment.
        .find(|(id, _, _)| brain.foreign_ready_makers.contains(id))
        .or_else(|| makers.iter()
            .find(|(entity_id, _, _)| brain.retrieve_makers.contains(entity_id)))
        .or_else(|| makers.first())
        .copied();
    let Some((entity_id, cell, yard)) = picked else {
        return;
    };
    let risk = super::site_risk::YardSurvey::read(rules,obs,yard,Some(entity_id));
    // Reserve a durable deployment job. The plot is kept across native Move
    // interruptions, and the yard receipt is checked when this maker vanishes.
    let intended = brain.maker_journeys.get(&entity_id)
        .map_or(cell, |(site, _, _)| *site);
    brain.tracked.tasks.yard_site(obs.tick(), entity_id, intended, cell);
    if let Some(type_id) = obs.type_id(&yard.id) {
        brain.tracked.tasks.yard_type(entity_id, type_id);
    }
    // A yard already stands, so this maker is not the way back from a loss: it
    // is the second base. It walks out and deploys where the house has ground
    // of its own.
    if yard_stands(&brain.census) {
        // But it walks home first, whatever it is standing on. A maker that came
        // out of a factory is near home and safe; a maker that was *captured*
        // is standing in the enemy's ground, and deploying there would be a
        // second base inside their guns. The wishlist's "pack it, drive it
        // home, cover the withdrawal" is this one condition.
        let space = super::terrain::Space::read(
            obs,
            rules,
            &brain.census.home_cells,
            brain.census.base_cell,
        );
        let home = brain.census.base_cell;
        let ground = space.ground_at(cell);
        // Only the enemy's own ground, or open contested ground away from home,
        // moves a maker. An enemy at the gate makes the cells around the base
        // Contested, and reading that as "flee" sent the house's own MCV
        // shuffling around its own walls under fire — the wishlist's
        // "主基地车会收缩".
        //
        // The radius is in cells, because `cell_distance` is Chebyshev. The
        // previous spelling multiplied `EXPAND_MIN_CELLS` by itself and by four,
        // which reads as a 400-cell radius on a map that is under 200 wide: `far`
        // was never true, so no maker was ever asked to come home and the arm
        // below only ever ran on the `exposed` half.
        let beyond_the_line =
            home.is_some_and(|home| cell_distance(cell, home) > EXPAND_MIN_CELLS * 2);
        let exposed = matches!(ground, super::terrain::Ground::Enemy)
            || (matches!(ground, super::terrain::Ground::Contested) && beyond_the_line);
        let far = beyond_the_line;
        // A maker that came from outside — captured, or out of a crate — has one
        // errand, and it is not "found a second base here".
        //
        // [`deploy_second_yard`] asks where a second base goes: away from home,
        // buildable, admitted. A captured maker standing beside the enemy's own
        // yard answers all three, so it deployed there; and because the cell it
        // admires changes as the admission flips, it walked between two answers
        // first. The wishlist's "占领后的 MCV 反复踱步，然后在野外展开了".
        //
        // The two kinds of maker are told apart by where they were first seen, not
        // by where they are now: the house's own maker appears at its own factory,
        // which is inside the base, and a captured one appears in the enemy's
        // base. `far` cannot make that distinction — the second base's maker is
        // meant to be far, and that is the whole point of it.
        //
        // The intent is sticky: once a maker is recalled it keeps coming home
        // even through the house's own safe ground. Releasing it the moment it
        // reaches safety hands it straight back to the second-base search, which
        // is the same walk again, one turn of the loop later.
        // The sighting test ran above for every maker; this one only repeats it
        // for the maker the selection picked, which `seen_makers` makes a no-op.
        let at_home = home.is_some_and(|home| cell_distance(cell, home) <= RETRIEVE_ARRIVED_CELLS);
        if brain.seen_makers.insert(entity_id) && !at_home {
            brain.retrieve_makers.insert(entity_id);
            log::info!("auto play: maker {entity_id} first seen at {cell:?}, recalling it");
        }
        // The one line that says why a maker is not coming home, on a term: a
        // report of "the captured MCV never arrived" needs to distinguish "it was
        // never asked", "it was asked and refused" and "it is walking".
        if obs.tick() % 600 == 0 {
            log::info!(
                "auto play: maker at {cell:?} home={home:?} exposed={exposed} far={far} dest={:?}",
                obs.destination(entity_id)
            );
        }
        // The walk home, and no other errand until it is home.
        //
        // `deploy_second_yard` is what a maker runs when the house wants a second
        // base, and a captured maker is not that maker: it must reach the first
        // base before anything else, or the house pays 2500 credits to hand the
        // enemy a yard beside their own.
        let dest = obs.destination(entity_id);
        let walking_away = dest.is_some_and(|(dx, dy)| {
            home.is_some_and(|home| cell_distance((dx, dy), home) > cell_distance(cell, home))
        });
        let foreign = super::build::foreign_yard(rules, obs, yard);
        if foreign && (at_home || brain.foreign_ready_makers.contains(&entity_id)) {
            // Once inside the recall radius the technology deployment phase
            // owns the MCV until an actual construction yard replaces it.
            // Never return to the recall/second-base branch merely because
            // an available foundation lies beyond RETRIEVE_ARRIVED_CELLS.
            brain.foreign_ready_makers.insert(entity_id);
            brain.retrieve_makers.remove(&entity_id);
            // Once this other-side maker reaches home, it should become a
            // yard *here*, never an outpost in enemy territory. Keep a stable
            // deployment plot in the main base until it fits or stalls.
            if let Some((site, old, progress)) = brain.maker_journeys.get(&entity_id).copied() {
                let progressed = if old != cell { obs.tick() } else { progress };
                // Let the final risk arbiter cancel the still-live native trip.
                if !risk.site(&brain.census,Some(cell),site) { return; }
                if maker_trip_stalled(obs.tick(), progressed)
                    || !obs.entity_type(rules, entity_id).is_some_and(|ty|
                        obs.can_reach(ty.movement_zone, cell, site)) {
                    brain.maker_journeys.remove(&entity_id);
                } else if cell_distance(cell, site) > 1 {
                    brain.maker_journeys.insert(entity_id, (site, cell, progressed));
                    if obs.destination(entity_id).is_none() {
                        super::orders::issue(brain, out, rules, obs,
                            entity_id, Goal::Move { cell: site });
                    }
                    return;
                } else if cell != site && obs.destination(entity_id) == Some(site) {
                    // The native Move is still closing its final one-cell
                    // approach; do not select another pad under its wheels.
                    brain.maker_journeys.insert(entity_id, (site, cell, progressed));
                    return;
                } else {
                    brain.maker_journeys.remove(&entity_id);
                }
            }
            if obs.deploy_site_admits(rules, entity_id, yard)
                && risk.site(&brain.census,Some(cell),cell) {
                // A deploy request is not proof of a finished construction
                // yard. Retain this task until the actual native building
                // appears, but do not restart Deploy every simulation frame.
                if (!yard_committed(brain, obs.tick(), entity_id)
                        || brain.tracked.tasks.yard_retry_due(obs.tick(), entity_id))
                    && super::orders::issue(brain, out, rules, obs, entity_id, Goal::Deploy)
                {
                    brain.tracked.tasks.yard_requested(obs.tick(), entity_id);
                    brain.yard_commit.insert(entity_id, obs.tick());
                    brain.retrieve_makers.remove(&entity_id);
                }
                return;
            }
            // Consider only foundations still inside the original base
            // perimeter. Never fall back to a remote second-base probe.
            // A crowded base may have no clear foundation within six cells
            // of the current vehicle. Search around the home anchor as a
            // second pass: it never authorizes expansion into enemy ground,
            // but it can locate a genuine second build pad behind the walls.
            let fits = |site: (u16, u16)| {
                home.is_some_and(|anchor| cell_distance(site, anchor) <= 18)
                    && !matches!(space.ground_at(site), super::terrain::Ground::Enemy)
                    && risk.site(&brain.census,Some(cell),site)
                    && obs.deploy_admits_at(rules, entity_id, yard, site)
                    && obs.entity_type(rules, entity_id).is_some_and(|ty|
                        obs.can_reach(ty.movement_zone, cell, site))
            };
            let near = rings(cell).find(|site| fits(*site))
                // The wider search is throttled: it can examine >1,000
                // cells, so do not run it on every 120-house brain tick.
                .or_else(|| (obs.tick() % 60 == 0).then(|| home.and_then(|anchor|
                    rings_to(anchor, 18).find(|site| fits(*site)))).flatten());
            if let Some(site) = near {
                if super::orders::issue(brain, out, rules, obs,
                    entity_id, Goal::Move { cell: site }) {
                    brain.maker_journeys.insert(entity_id, (site, cell, obs.tick()));
                }
            } else {
                clear_mcv_foundation(brain, rules, obs, out, cell, entity_id);
            }
            return;
        }
        if brain.retrieve_makers.contains(&entity_id) {
            let Some(home) = home else {
                // No base to come home to: the maker waits rather than deploying
                // into ground the house does not hold.
                return;
            };
            if cell_distance(cell, home) <= RETRIEVE_ARRIVED_CELLS {
                brain.retrieve_makers.remove(&entity_id);
                log::info!("auto play: maker home at {cell:?}");
            } else {
                // The rally cell, and the base itself when every ring around it
                // is built over: a maker with nowhere to stand is a maker that
                // never arrives.
                let target =
                    super::census::rally_near(obs, home, &brain.census.home_cells).unwrap_or(home);
                if target != cell
                    && (dest.is_none() || walking_away)
                    && !super::orders::issue(
                        brain,
                        out,
                        rules,
                        obs,
                        entity_id,
                        Goal::Move { cell: target },
                    )
                {
                    // The walk was refused, so the maker stays where it is and
                    // the next frame asks again: clearing the retrieve entry here
                    // would only send it out as a second base on ground it cannot
                    // even reach home from.
                    log::info!("auto play: maker {entity_id} cannot reach home from {cell:?}");
                }
                return;
            }
        }
        deploy_second_yard(brain, rules, obs, out, entity_id, cell, yard);
        return;
    }
    // At skirmish start all the gifted infantry may spawn around the MCV.
    // Clear the *existing* foundation first, before moving the MCV to another
    // plot: the MCV could legally deploy right here once the infantry step off.
    // This runs only when there is no yard yet and when our own mobile infantry
    // are currently beside the maker, not on a blocked enemy/remote plot.
    if !yard_stands(&brain.census)
        && !obs.deploy_site_admits(rules, entity_id, yard)
        && obs.destination(entity_id).is_none_or(|dest|
            cell_distance(cell, dest) <= 2)
        && brain.census.units.iter().any(|inf| inf.traits.combat
            && inf.slot == super::census::Slot::Infantry && inf.controllable()
            && super::squared_distance(inf.cell, cell) <= 3 * 3)
        && clear_mcv_foundation(brain, rules, obs, out, cell, entity_id) > 0
    {
        return;
    }
    // The *first* MCV also needs its own appointment. Without a yard, the
    // placement sweep is anchored on the mobile base; as it walks the anchor
    // moves and the probe chooses yet another foundation. Keep the original
    // legal plot across frames and resume an interrupted Move instead of
    // recomputing it from the MCV's new position every tick.
    if !yard_stands(&brain.census)
        && let Some((site, last_cell, last_progress)) =
            brain.maker_journeys.get(&entity_id).copied()
    {
        let progress = if cell != last_cell { obs.tick() } else { last_progress };
        let reachable = obs.entity_type(rules, entity_id)
            .is_some_and(|ty| obs.can_reach(ty.movement_zone, cell, site));
        if !risk.site(&brain.census,Some(cell),site) { return; }
        if maker_trip_stalled(obs.tick(), progress) || !reachable {
            brain.maker_journeys.remove(&entity_id);
            if let Some(id) = obs.type_id(&yard.id) {
                let attempt = brain.maker_site_attempts.get(&id).copied().unwrap_or(0);
                brain.maker_site_attempts.insert(id,
                    (attempt + 1) % PLACEMENT_ATTEMPTS);
            }
            log::debug!("auto play: first MCV {entity_id} rejects stalled plot {site:?}");
        } else if cell_distance(cell, site) > 2 {
            brain.maker_journeys.insert(entity_id, (site, cell, progress));
            if obs.destination(entity_id).is_some() { return; }
            if super::orders::issue(brain, out, rules, obs,
                entity_id, Goal::Move { cell: site }) { return; }
            brain.maker_journeys.remove(&entity_id);
        } else {
            brain.maker_journeys.remove(&entity_id);
        }
    }
    // A fresh MCV already travelling to a deployable plot must finish that
    // journey. Rechecking Placement every tick while the plot is transiently
    // clear was a source of A-B-A walks and premature roadside deployments.
    if obs.destination(entity_id).is_some_and(|dest|
        cell_distance(cell, dest) > 2) {
        return;
    }
    if obs.deploy_site_admits(rules, entity_id, yard)
        && risk.site(&brain.census,Some(cell),cell) {
        // The one guard the pack also runs: a yard the house only just packed
        // home is inside its term, and deploying it again is the round trip the
        // commitment exists to stop.
        if (!yard_committed(brain, obs.tick(), entity_id)
                || brain.tracked.tasks.yard_retry_due(obs.tick(), entity_id))
            && super::orders::issue(brain, out, rules, obs, entity_id, Goal::Deploy)
        {
            brain.tracked.tasks.yard_requested(obs.tick(), entity_id);
            brain.yard_commit.insert(entity_id, obs.tick());
        }
        return;
    }
    let Some(yard_id) = obs.type_id(&yard.id) else {
        return;
    };
    // The sweep's cells are the sidebar's own build area, so the maker ends up
    // on base ground the way a player would put it. `maker_site_attempts` is what
    // moves past a cell that cannot be used, the same memory a waiting
    // structure uses.
    let attempt = brain.maker_site_attempts.get(&yard_id).copied().unwrap_or(0);
    match Placement::new(obs, rules).cell_for(yard_id, attempt) {
        Some(target) if target != cell && risk.site(&brain.census,Some(cell),target) => {
            if obs.entity_type(rules, entity_id)
                .is_some_and(|ty| obs.can_reach(ty.movement_zone, cell, target))
            {
                if super::orders::issue(
                    brain, out, rules, obs, entity_id,
                    Goal::Move { cell: target },
                ) {
                    brain.maker_journeys.insert(entity_id,
                        (target, cell, obs.tick()));
                }
            } else {
                brain.maker_site_attempts.insert(yard_id,
                    (attempt + 1) % PLACEMENT_ATTEMPTS);
            }
        }
        _ => {
            // The sweep's candidate is the cell the maker already stands on, or
            // there is none. Advancing the counter alone is what left a maker
            // standing for a whole match with nothing issued: measured, an MCV
            // whose deploy footprint is obstructed reads `admits=false`, stays
            // in the census, and got `orders=0` for every attempt.
            //
            // The origin of a deploy is the foundation's corner, not the unit's
            // cell (`deploy_origin_from_unit_cell` shifts north-west for a
            // foundation wider than two), so the cell that has to be free is a
            // neighbour's. `deploy_admits_at` answers about the cell the unit
            // would stand on, so the walk out is a ring search for the first
            // cell the deploy would land on.
            let space = super::terrain::Space::read(
                obs,
                rules,
                &brain.census.home_cells,
                brain.census.base_cell,
            );
            match nearest_deploy_site(obs, rules, &space, &brain.census, &risk, entity_id, yard, cell) {
                Some(target) => {
                    if super::orders::issue(
                        brain,
                        out,
                        rules,
                        obs,
                        entity_id,
                        Goal::Move { cell: target },
                    ) {
                        // Also lock fallback plots. Otherwise placement can
                        // choose A/B/A plots while the first MCV is walking.
                        brain.maker_journeys.insert(entity_id,
                            (target, cell, obs.tick()));
                    }
                }
                None => {
                    let next = (attempt + 1) % PLACEMENT_ATTEMPTS;
                    brain.maker_site_attempts.insert(yard_id, next);
                    // Walled in on every side: the counter alone is what left a
                    // maker standing for a whole match. There is nowhere to
                    // deploy here, so the maker goes back to the base it still
                    // has — the same rally walk the retreat uses — and tries
                    // again from ground the house actually holds.
                    if let Some(home) = brain.census.base_cell
                        && cell_distance(cell, home) > RETRIEVE_ARRIVED_CELLS
                        && obs.destination(entity_id).is_none()
                    {
                        let target = super::census::rally_near(obs, home, &brain.census.home_cells)
                            .unwrap_or(home);
                        super::orders::issue(
                            brain,
                            out,
                            rules,
                            obs,
                            entity_id,
                            Goal::Move { cell: target },
                        );
                    }
                }
            }
        }
    }
}

/// The nearest cell the maker could stand on and deploy, by rings outwards.
///
/// Bounded by [`DEPLOY_SITE_RINGS`] so a maker walled in on every side reports
/// "nowhere" rather than walking the map.
fn nearest_deploy_site(
    obs: &Observation<'_>,
    rules: &RuleSet,
    space: &super::terrain::Space,
    census: &Census,
    risk: &super::site_risk::YardSurvey,
    entity_id: u64,
    yard: &ObjectType,
    from: (u16, u16),
) -> Option<(u16, u16)> {
    let zone = obs.entity_type(rules, entity_id).map(|ty| ty.movement_zone);
    let fits = |cell: (u16, u16)| risk.site(census,Some(from),cell)
        && obs.deploy_admits_at(rules, entity_id, yard, cell)
        && zone.is_none_or(|zone| obs.can_reach(zone, from, cell));
    // Ground the house may build on first, any cell the deploy physically fits
    // on second. `deploy_admits_at` answers whether the footprint is clear, not
    // whether the ground is worth standing on: taking its answer alone walked a
    // maker straight into the enemy's own base, which is the wishlist's
    // "冲进敌人家里". The second pass is what keeps a maker that has nowhere
    // good left from standing still forever.
    rings(from)
        .filter(|cell| fits(*cell) && space.ground_at(*cell).buildable())
        .next()
        .or_else(|| rings(from).filter(|cell| fits(*cell)).next())
}

/// Cells by rings outwards from `from`, bounded by [`DEPLOY_SITE_RINGS`], in the
/// one order that makes the answer the same every frame.
fn rings(from: (u16, u16)) -> impl Iterator<Item = (u16, u16)> {
    rings_to(from, DEPLOY_SITE_RINGS)
}

/// Extended only for the technology yard that must settle near its home base.
fn rings_to(from: (u16, u16), limit: i32) -> impl Iterator<Item = (u16, u16)> {
    (1..=limit).flat_map(move |ring| {
        let mut cells = Vec::new();
        for dy in -ring..=ring {
            for dx in -ring..=ring {
                if dx.abs().max(dy.abs()) != ring {
                    continue;
                }
                let cell = (from.0 as i32 + dx, from.1 as i32 + dy);
                if cell.0 < 0 || cell.1 < 0 {
                    continue;
                }
                cells.push((cell.0 as u16, cell.1 as u16));
            }
        }
        cells.sort_unstable();
        cells
    })
}

/// How far from the maker a deploy site may be searched, in cells.
const DEPLOY_SITE_RINGS: i32 = 6;

/// Whether the house should be buying a yard maker it does not need yet: one
/// for a second base.
///
/// Native's expansion is `AI_Manage_Build_Queue`'s, which is not ported; the
/// gates here are what any house has to satisfy before a maker is worth a war
/// factory cycle — a standing yard (otherwise it is the way back from a loss,
/// which [`super::composition`] already answers), the trucks to fill the bays
/// it has, and a wallet that can still build once the maker is paid for. The
/// type itself comes from the same `CanBuild` answer the sidebar reads
/// ([`Self::yard_maker`]), so a house that cannot build one simply does not.
pub(super) fn wants_a_maker(
    brain: &mut AssistBrain,
    rules: &RuleSet,
    obs: &Observation<'_>,
    budget: &Budget,
) -> bool {
    // The same precondition the saving uses, so the house that is holding money
    // for a maker is exactly the house allowed to buy one. Two different tests
    // here is how the wallet and the purchase miss each other.
    if !expansion_is_worth_saving_for(brain, rules, obs) {
        return false;
    }
    // And ground to put it on. A maker that cannot deploy is a maker the
    // house paid 2500 credits for and then watches stand in the factory's
    // doorway for the rest of the match, so the house only buys one when
    // there is a patch of ground a second yard fits on.
    let Some(yard) = yard_type(rules, obs) else {
        return false;
    };
    // Without a map there is no answer, and no answer is not a refusal: the
    // fixtures and the headless harness run without one.
    if obs.map_extent().0 > 0 && expansion_site(brain, rules, obs, yard, None).is_none() {
        return false;
    }
    // The wallet, against what the maker costs rather than a fixed 4000.
    //
    // The fixed floor was a second self-lock of the same shape as the infantry
    // ceiling's: measured on Arena.mmx seed 0x5EED, the wallet peaked at 4550
    // on tick 3600 — before any harvester existed — and then ran between 400
    // and 2400 for the whole match, because the factories spend everything they
    // are given. So `harvesters_served` and `free >= 4000` were never true on
    // the same frame, and the house never bought a maker at all: three
    // `second base site` lines and no maker in any roster.
    //
    // The maker's own price is the real question. And the house does not wait
    // for the wallet to reach it on its own: `build::spend` holds the
    // difference back (`yard_reserve`) the moment the precondition above
    // answers true, so the wallet fills instead of being spent by the lines.
    budget.free >= expansion_price(rules, obs)
}

/// The most yards a house may stand without a reason for another.
const MAX_YARDS: usize = 2;

/// How far ore has to stand from the base before it justifies a base of its own,
/// in cells: past the ring the first base already covers.
const FAR_ORE_CELLS: i32 = 24;

/// How many frames a far-ore answer stands before it is looked up again.
const FAR_ORE_FRAMES: u64 = 600;

/// Whether ore stands beyond [`FAR_ORE_CELLS`] of the house's base.
///
/// A whole-map walk, so it is only reached when the yard cap is already met —
/// that is, on the frames a house with two bases is deciding whether a third is
/// worth 2500 credits. Not a per-frame question.
fn far_ore(known: &std::collections::BTreeMap<(u16, u16), u8>, home: (u16, u16)) -> bool {
    known.keys()
        .any(|cell| super::squared_distance(*cell, home) > FAR_ORE_CELLS * FAR_ORE_CELLS)
}

/// Whether a second base is wanted at all, ignoring the wallet.
///
/// The half of [`wants_a_maker`] that answers "would the house buy a maker if
/// it had the money". It exists because the reserve and the purchase ask
/// opposite halves of the same question: `build::spend` holds money back when
/// the house *is* saving, and `wants_a_maker` buys when the saving is
/// finished. One predicate for both would be a house that never saves because
/// it never has enough.
pub(super) fn expansion_is_worth_saving_for(
    brain: &AssistBrain,
    rules: &RuleSet,
    obs: &Observation<'_>,
) -> bool {
    // An optional second yard must have strategic permission. This is not
    // the lost-yard recovery path, which still buys its MCV unconditionally.
    if !brain.director.may_expand() || brain.mobilization.active() || brain.campaign.economy_resting() {
        return false;
    }
    if brain.director.initialized() && (brain.census.refineries == 0
        || brain.census.harvesters < 2 || brain.census.available_fighters() < 8
        || brain.census.base_cell.is_none_or(|home| !far_ore(&brain.ore_known,home))) {
        return false;
    }
    if !yard_stands(&brain.census) || owns_yard_maker(&brain.census) {
        return false;
    }
    // A second base is the ordinary case; a third needs a reason, and the reason
    // is the wishlist's "矿点距离": ore worth a base of its own, standing beyond
    // the ring the first base covers. A house that opens bases with no ore in
    // them has bought 2500 credits of ground it cannot use.
    //
    // The ore walk is a whole-map scan, so it is cached and refreshed on a slow
    // cadence rather than asked per frame.
    if brain.census.yards > MAX_YARDS {
        return false;
    }
    let too_many = brain.census.yards >= MAX_YARDS;
    if too_many {
        let Some(home) = brain.census.base_cell else {
            return false;
        };
        if !far_ore(&brain.ore_known, home) {
            return false;
        }
    }
    // The first base has to exist, and that is the whole test.
    //
    // An earlier arm also asked `harvesters_served` and then "one truck and one
    // bay", and the log says why neither worked: `harv=0 ref=0` holds for the
    // first ~1800 frames while the wallet falls from 10000 to 5000, so every
    // frame with money in it was a frame where the precondition refused. By the
    // time a truck existed the factories had spent the opening, and the price
    // was never reached again.
    //
    // A second base is bought out of the opening's own credits or not at all,
    // and holding part of them back is exactly what `yard_reserve` is for. The
    // manufactory gate (`ExpansionNeeds`) is the real question: `AMCV` carries
    // `Prerequisite=GAWEAP,GADEPT`, so a house without a service depot cannot
    // buy one whatever it holds, and `expansion_site` already refuses when the
    // rules offer no maker.
    yard_type(rules, obs).is_some()
}

/// What a second base costs before it produces anything: the maker plus the
/// first structure that follows it out of the new yard.
pub(super) fn expansion_price(rules: &RuleSet, obs: &Observation<'_>) -> i32 {
    let maker = super::build::all_class_types(rules, obs, ObjectCategory::Vehicle)
        .filter(|(ty, _)| makes_a_yard(rules, ty))
        .map(|(ty, _)| obs.cost_of(rules, ty))
        .min()
        .unwrap_or(EXPAND_MIN_CREDITS);
    (maker + EXPAND_FIRST_STRUCTURE).max(EXPAND_MIN_CREDITS / 2)
}

/// The yard type the house's maker deploys into, when the house has one.
fn yard_type<'r>(rules: &'r RuleSet, obs: &Observation<'_>) -> Option<&'r ObjectType> {
    super::build::all_class_types(rules, obs, ObjectCategory::Vehicle)
        .filter(|(ty, _)| makes_a_yard(rules, ty))
        .filter_map(|(maker, _)| {
            rules
                .object(maker.deploys_into.as_deref()?)
                .filter(|yard| yard.build_const_eligible())
        })
        .next()
}

/// Drive a maker the house bought for a second base out, and deploy it there.
///
/// A second yard beside the first is not an expansion, it is the same base with
/// more of it — the sweep's own job is to keep one base from becoming a block,
/// so the second one is started far enough away to be its own.
fn deploy_second_yard(
    brain: &mut AssistBrain,
    rules: &RuleSet,
    obs: &Observation<'_>,
    out: &mut Outbox,
    entity_id: u64,
    cell: (u16, u16),
    yard: &ObjectType,
) {
    let Some(home) = brain.census.base_cell else {
        return;
    };
    // Standing on ground that takes a yard, far enough from home to be a
    // second base: that is the player's own D press.
    //
    // The maker does not have to reach the cell the house picked: any ground it
    // passes that takes a yard is ground it can take, and a maker that walks
    // past a dozen of them and keeps looking for the one it was told about
    // never deploys at all.
    //
    // The ground also has to be *ours* to build on. The caller's flee arranges
    // that, but a maker that stopped running one cell later must not deploy
    // there anyway: that is the wishlist's "占领敌方MCV 收缩后又原地展开".
    let space = super::terrain::Space::read(obs, rules, &brain.census.home_cells, Some(home));
    let risk=super::site_risk::YardSurvey::read(rules,obs,yard,Some(entity_id));
    // The selected ore-side plot, not merely "somewhere ten cells from home",
    // owns the deploy decision. An MCV that stops short of its target used to
    // unfold at the first legal roadside cell and later get packed again.
    // One selected plot belongs to this *MCV*, not the global expansion
    // survey. An active journey holds its destination even after the survey's
    // 450-frame cache expires. Only lack of actual progress invalidates it.
    let selected = if let Some((site, last_cell, progress)) =
        brain.maker_journeys.get(&entity_id).copied()
    {
        let progress = if cell != last_cell { obs.tick() } else { progress };
        let reachable = obs.entity_type(rules, entity_id).is_some_and(|ty|
            obs.can_reach(ty.movement_zone, cell, site));
        if !risk.site(&brain.census,Some(cell),site) { return; }
        if brain.blocked_expansion_sites.contains_key(&site)
            || !reachable || maker_trip_stalled(obs.tick(), progress)
        {
            brain.maker_journeys.remove(&entity_id);
            brain.blocked_expansion_sites.insert(site,
                obs.tick().saturating_add(MCV_REJECT_SITE_FRAMES));
            brain.expansion_probe = None;
            log::info!("auto play: MCV {entity_id} abandons stalled expansion plot {site:?}");
            expansion_site(brain, rules, obs, yard, Some(entity_id))
        } else {
            brain.maker_journeys.insert(entity_id, (site, cell, progress));
            Some(site)
        }
    } else {
        expansion_site(brain, rules, obs, yard, Some(entity_id))
    };
    // An MCV can arrive successfully but be unable to unfold because the
    // native foundation is blocked by friendly infantry. Do not bounce
    // between two probe answers: clear a local route first, then mark the
    // failed plot unavailable for a bounded period and pick an alternate.
    if let Some(site) = selected
        && cell_distance(cell, site) <= 2
        && obs.destination(entity_id).is_none_or(|dest|
            cell_distance(cell, dest) <= 2)
        && !obs.deploy_site_admits(rules, entity_id, yard)
    {
        let since = brain.maker_obstructed.entry(entity_id)
            .or_insert((site, obs.tick()));
        if since.0 != site { *since = (site, obs.tick()); }
        if retry_blocked_site(obs.tick(), since.1) {
            clear_mcv_foundation(brain, rules, obs, out, site, entity_id);
            return;
        }
        brain.blocked_expansion_sites.insert(site,
            obs.tick().saturating_add(MCV_REJECT_SITE_FRAMES));
        brain.maker_obstructed.remove(&entity_id);
        brain.maker_journeys.remove(&entity_id);
        brain.expansion_probe = None;
        log::debug!("auto play: MCV {entity_id} failed at {site:?}; selecting alternate plot");
        return;
    } else {
        brain.maker_obstructed.remove(&entity_id);
    }
    if at_selected_site(cell, selected)
        && obs.destination(entity_id).is_none_or(|dest|
            dest == cell || selected == Some(dest))
        && cell_distance(cell, home) >= EXPAND_MIN_CELLS
        && space.ground_at(cell) != super::terrain::Ground::Enemy
        && risk.site(&brain.census,Some(cell),cell)
        && obs.deploy_site_admits(rules, entity_id, yard)
    {
        if (!yard_committed(brain, obs.tick(), entity_id)
                || brain.tracked.tasks.yard_retry_due(obs.tick(), entity_id))
            && super::orders::issue(brain, out, rules, obs, entity_id, Goal::Deploy)
        {
            brain.tracked.tasks.yard_requested(obs.tick(), entity_id);
            brain.yard_commit.insert(entity_id, obs.tick());
            brain.maker_journeys.remove(&entity_id);
        }
        return;
    }
    // The cell the house decided on: a patch of ground a second yard fits on,
    // away from the first base. The same probe the purchase gate asked, so the
    // maker walks to ground the house already knows about.
    let Some(target) = selected else {
        // No ground for a second yard. The maker still has to leave the
        // factory: a vehicle standing in the doorway holds the whole vehicle
        // line, so "nowhere to expand" must not mean "stand where it was made"
        // — that is the wishlist's "the MCV sits in the war factory".
        //
        // It walks to the rally cell the retreat already uses, which is a cell
        // *beside* the base rather than inside it. The maker is not spent: the
        // next frame asks the search again, and a house that finds ground later
        // still deploys.
        if obs.destination(entity_id).is_some() {
            return;
        }
        let Some(home) = brain.census.base_cell else {
            return;
        };
        let Some(rally) = super::census::rally_near(obs, home, &brain.census.home_cells) else {
            return;
        };
        if cell_distance(rally, cell) <= 2 {
            return;
        }
        if !super::orders::issue(
            brain,
            out,
            rules,
            obs,
            entity_id,
            Goal::Move { cell: rally },
        ) {
            brain.expansion_probe = None;
        }
        return;
    };
    // A maker already walking is a maker whose cell has been chosen: the
    // walk is over the map's ground and costs a foundation test per cell, so
    // it is asked when the unit stands still and not once a frame while it
    // drives.
    if obs.destination(entity_id).is_some() {
        return;
    }
    if cell != target {
        if super::orders::issue(
            brain, out, rules, obs, entity_id, Goal::Move { cell: target },
        ) {
            brain.maker_journeys.insert(entity_id, (target, cell, obs.tick()));
        } else {
            // A rejected plot stays rejected for a bounded time; otherwise
            // the next planning tick would immediately reverse to it.
            brain.blocked_expansion_sites.insert(target,
                obs.tick().saturating_add(MCV_REJECT_SITE_FRAMES));
            brain.maker_journeys.remove(&entity_id);
            brain.expansion_probe = None;
        }
    }
}

/// A long trip remains committed while its actual position changes; only a
/// sustained lack of progress permits a different plot. Frames, not seconds.
fn maker_trip_stalled(now: u64, last_progress: u64) -> bool {
    now.saturating_sub(last_progress) >= 360
}

/// Bounded retry: clear incidental blockers without leaving a maker parked
/// indefinitely on a plot the native deploy will never accept.
fn retry_blocked_site(now: u64, first_blocked: u64) -> bool {
    now.saturating_sub(first_blocked) < MCV_CLEARANCE_FRAMES
}

fn clear_mcv_foundation(
    brain: &mut AssistBrain, rules: &RuleSet,
    obs: &Observation<'_>, out: &mut Outbox,
    site: (u16,u16), maker: u64,
) -> usize {
    let mut moved = 0;
    // A new match may start with ten or more infantry blocking the only yard.
    // Clear them in one bounded batch, rather than two per long task period.
    let limit = if yard_stands(&brain.census) { 2 } else { 16 };
    for infantry in &brain.census.units {
        if moved >= limit { break; }
        if infantry.entity_id == maker
            || infantry.slot != super::census::Slot::Infantry
            || !infantry.controllable() || infantry.in_service()
            || super::squared_distance(infantry.cell, site) > 5*5
            || out.ordered_this_frame(infantry.entity_id)
            || brain.tracked.held(obs.tick(), infantry.entity_id)
            || obs.attack_target(infantry.entity_id).is_some() { continue; }
        let Some(ty) = obs.entity_type(rules, infantry.entity_id) else { continue; };
        // Walk outward from the blocked footprint. Candidates are bounded
        // and checked against the real native movement grid and live threats.
        let offsets = [(7,0),(-7,0),(0,7),(0,-7),(6,6),(6,-6),(-6,6),(-6,-6)];
        let target = offsets.into_iter().filter_map(|(dx,dy)| {
            let (Ok(x),Ok(y)) = (u16::try_from(i32::from(site.0)+dx),
                u16::try_from(i32::from(site.1)+dy)) else { return None; };
            let new_cell = (x,y);
            (obs.is_cell_in_playfield(x,y) && obs.is_ground_walkable(x,y)
                && obs.can_reach(ty.movement_zone, infantry.cell, new_cell)
                && !obs.visible_enemy_entities().any(|enemy|
                    super::squared_distance(enemy.cell,new_cell) <= 7*7))
                .then_some(new_cell)
        }).min_by_key(|cell| super::squared_distance(infantry.cell,*cell));
        let Some(target) = target else { continue; };
        if super::orders::issue_tracked(&mut brain.tracked, brain.trace,
            out, rules, obs, infantry.entity_id,
            super::tracked::TrackedKind::Avoid, None, Some(target),
            Goal::Move { cell: target }) { moved += 1; }
    }
    moved
}

/// A unit blocking the foundation on a single frame is not a good reason to
/// abandon a journey to a mineral field. Reachability is a genuine veto;
/// transient deploy admission is allowed a bounded re-check interval.
fn keep_expansion_appointment(
    now: u64, chosen_at: u64, reachable: bool, admits: bool,
) -> bool {
    reachable && (admits || now.saturating_sub(chosen_at) < EXPANSION_SITE_COMMIT_FRAMES)
}

/// A roadside cell is not an expansion site. Keep the selected site stable
/// across movement and only unfold after arriving within placement tolerance.
fn at_selected_site(cell: (u16, u16), site: Option<(u16, u16)>) -> bool {
    site.is_some_and(|site| cell_distance(cell, site) <= 2)
}

/// The nearest cell a second yard may be deployed on: away from the house's own
/// base, on ground a unit can stand on, and a cell a yard of this type fits on.
///
/// The rings are walked outwards from the base and the first admissible cell in
/// the fixed ring order wins, so the walk is the shortest one that gets the
/// maker clear of what the house has already built. Ore is not a factor yet:
/// the rules' overlay is not reachable from [`Observation`], and guessing at a
/// field from the truck's path is not evidence.
pub(super) fn expansion_site(
    brain: &mut AssistBrain,
    rules: &RuleSet,
    obs: &Observation<'_>,
    yard: &ObjectType,
    maker: Option<u64>,
) -> Option<(u16, u16)> {
    let tick = obs.tick();
    brain.blocked_expansion_sites.retain(|_,until| *until > tick);
    let rejected: std::collections::BTreeSet<_> =
        brain.blocked_expansion_sites.keys().copied().collect();
    // The deploy's own answer for this cell. With a maker on the field the
    // question is asked with the maker's body lifted -- the deploy lifts it
    // -- and without one the ground has to be clear of everything.
    let maker_position = maker.and_then(|id| {
        brain.census.units.iter().find(|unit| unit.entity_id == id).and_then(|unit| {
            let ty = obs.object_type(rules, unit.type_id)?;
            Some((unit.cell, ty.movement_zone))
        })
    });
    let risk=super::site_risk::YardSurvey::read(rules,obs,yard,maker);
    let admits = |cell: (u16, u16)| {
        if rejected.contains(&cell) || !risk.site(&brain.census,maker_position.map(|p|p.0),cell) { return false; }
        let reachable = maker_position.is_none_or(|(from, zone)| obs.can_reach(zone, from, cell));
        reachable && match maker {
            Some(id) => obs.deploy_admits_at(rules, id, yard, cell),
            None => obs.clear_for_deploy(rules, yard, cell),
        }
    };
    // A cached answer is used while it still holds: the probe walks the
    // map's ground, and a maker that re-aims on every probe spends the match
    // walking between two patches instead of reaching one.
    if let Some((selected_at, Some(site))) = brain.expansion_probe {
        let reachable = maker_position.is_none_or(|(from, zone)| {
            obs.can_reach(zone, from, site)
        });
        // MCVs are large. A transient tank or harvester on the foundation
        // changes deploy_admits_at without making the ore field a bad plan.
        // Keep walking toward the committed site for a bounded period instead
        // of alternating between two newly admissible cells each frame.
        let travelling_to_site = maker.is_some_and(|id|
            obs.destination(id) == Some(site));
        if !rejected.contains(&site)
            && risk.site(&brain.census,maker_position.map(|p|p.0),site)
            && reachable
            && (travelling_to_site
                || keep_expansion_appointment(tick, selected_at, reachable, admits(site)))
        {
            return Some(site);
        }
    }
    if brain.expansion_probe.is_some_and(|(at,site)|site.is_none()
        && tick.saturating_sub(at)<EXPAND_PROBE_FRAMES) {return None;}
    let Some(home) = brain.census.base_cell else {
        brain.expansion_probe = Some((tick, None));
        return None;
    };
    // An MCV already on the map may outlive the distant ore that justified
    // its purchase. Never turn that into permission to deploy at a random
    // roadside fallback: hold the maker at a safe rally point until the
    // player has actually discovered another distant field.
    if brain.director.initialized()
        && expansion_aim(&brain.ore_known, home, &brain.census.home_cells).is_none()
    {
        brain.expansion_probe = Some((tick, None));
        return None;
    }
    let found = search_expansion_site(
        &brain.ore_known,
        obs,
        home,
        &brain.census.home_cells,
        yard,
        &admits,
        &super::terrain::Space::read(obs, rules, &brain.census.home_cells, brain.census.base_cell),
    );
    if found.is_some() {
        log::info!("auto play: second base site at {found:?}",);
    }
    brain.expansion_probe = Some((tick, found));
    found
}

/// Whether `cell` is far enough from the house's own buildings to be a
/// second base's yard.
///
/// A maker that deploys beside the first yard is the same base with more of
/// it, and the point of the walk is to leave.
fn is_a_second_base_cell(home_cells: &[(u16, u16)], cell: (u16, u16)) -> bool {
    home_cells
        .iter()
        .all(|home| cell_distance(*home, cell) >= EXPAND_MIN_CELLS)
}

/// The nearest cell a second yard may be deployed on: away from the house's
/// own base, on ground the yard fits, and clear of everything that stands
/// there.
///
/// Ore decides *where*, not whether. The wishlist's "占据矿脉，在矿脉附近建立
/// 分基地": the search runs outward from the nearest ore the house does not
/// already stand on, so the yard goes to the field it is meant to work. Only
/// when the map carries no such ore does it fall back to the old answer —
/// outward from the base, first patch that takes a yard — which is what a house
/// on a map with no visible field should do.
///
/// The search asks the deploy's own question -- the foundation's origin and
/// the clear-to-build test at that origin -- and not the sidebar's build-area
/// adjacency: a second base is outside the first base's build area by
/// definition, so asking the placement question answered No for every cell
/// and the maker stood where the factory put it.
fn search_expansion_site(
    known_ore: &std::collections::BTreeMap<(u16, u16), u8>,
    obs: &Observation<'_>,
    home: (u16, u16),
    home_cells: &[(u16, u16)],
    yard: &ObjectType,
    admits: &dyn Fn((u16, u16)) -> bool,
    space: &super::terrain::Space,
) -> Option<(u16, u16)> {
    let aim = expansion_aim(known_ore, home, home_cells);
    if let Some(ore) = aim {
        // A refinery should be close to its field. Starting this search at
        // EXPAND_MIN_CELLS used to skip every good cell right next to the ore
        // and could return a site over a dozen cells away. Distance from the
        // *existing base* is checked separately in search_rings_from.
        return search_rings_from(obs, home_cells, ore, 0, EXPAND_ORE_RING, admits, space);
    }
    // A few isolated visible ore tiles are not evidence for paying for an
    // entire new base. A fallback yard beside the original home consumes the
    // maker but never reaches the rich centre of the map.
    if !known_ore.is_empty() {
        return None;
    }
    search_rings_from(obs, home_cells, home, EXPAND_MIN_CELLS, EXPAND_SCAN_CELLS, admits, space)
}

/// The ore cell a new base should be built beside: the nearest one the house
/// does not already stand on.
///
/// A cell under an existing yard is ore the first base already works, so a
/// second base aimed at it would be a second base for the same field. Beyond
/// [`EXPAND_MIN_CELLS`] of every yard is the test, which is the same line the
/// site itself has to clear.
fn expansion_aim(
    known_ore: &std::collections::BTreeMap<(u16, u16), u8>,
    home: (u16, u16),
    home_cells: &[(u16, u16)],
) -> Option<(u16, u16)> {
    // The overlay grid knows hidden deposits. An honest player only knows
    // cells seen while visible; use the same last-seen survey as miner buying.
    // Prefer a substantial local seam over the first isolated ore tile on
    // the path. Each tile's cached stage estimates its remaining stock; live
    // hidden overlay data must never enter this estimate.
    known_ore.keys().copied()
        .filter(|cell| is_a_second_base_cell(home_cells, *cell))
        .filter_map(|cell| {
            let local_stock = known_ore.iter()
                .filter(|(near,_)| cell_distance(**near,cell) <= 6)
                .map(|(_,stage)| u32::from(*stage)+1)
                .sum::<u32>();
            (local_stock >= 8).then_some((cell,local_stock))
        })
        .max_by_key(|(cell,stock)| (*stock,
            std::cmp::Reverse(cell_distance(*cell,home)),
            std::cmp::Reverse(*cell)))
        .map(|(cell,_)| cell)
}

/// The first admissible cell in rings out from `origin`, up to `max_ring`.
///
/// Rings are walked outwards and the first cell that clears every test wins, so
/// the answer is the shortest walk from `origin` to ground that takes a yard.
fn search_rings_from(
    obs: &Observation<'_>,
    home_cells: &[(u16, u16)],
    origin: (u16, u16),
    min_ring: i32,
    max_ring: i32,
    admits: &dyn Fn((u16, u16)) -> bool,
    space: &super::terrain::Space,
) -> Option<(u16, u16)> {
    let (width, height) = obs.map_extent();
    // No map, no expansion: the headless harness and the fixtures run without
    // one, and a walk into cell (0, 0) is worse than staying put.
    if width == 0 || height == 0 {
        return None;
    }
    for ring in min_ring..=max_ring {
        let mut supported = None;
        for dy in -ring..=ring {
            for dx in -ring..=ring {
                if dx.abs().max(dy.abs()) != ring {
                    continue;
                }
                let (Some(rx), Some(ry)) = (
                    u16::try_from(i32::from(origin.0) + dx).ok(),
                    u16::try_from(i32::from(origin.1) + dy).ok(),
                ) else {
                    continue;
                };
                if rx >= width || ry >= height || !obs.is_ground_walkable(rx, ry) {
                    continue;
                }
                // Not inside a base the house already has: that is where the
                // first yard is, and the point of the walk is to leave it.
                if !is_a_second_base_cell(home_cells, (rx, ry)) {
                    continue;
                }
                // And the ground has to take a yard.
                if !admits((rx, ry)) {
                    continue;
                }
                // And it has to be ground the house can keep. A second yard
                // inside the enemy's reach is a yard they destroy for free,
                // and one inside their base is a yard they never let stand —
                // the wishlist's "new buildings appear in the enemy's face".
                // Read through the shared map reading rather than a second
                // distance rule of this file's own.
                // `admits` above has checked actual visible weapon cover and
                // nearby support. Contested ground is not automatically lethal.
                match space.ground_at((rx, ry)) {
                    super::terrain::Ground::Enemy => continue,
                    super::terrain::Ground::Contested => { supported.get_or_insert((rx,ry)); }
                    _ => return Some((rx,ry)),
                }
            }
        }
        // Prefer quieter ground at the same distance; a supported contested
        // site can still be worth the expansion instead of waiting forever.
        if supported.is_some() {return supported;}
    }
    None
}

/// The maker the house may buy now, or `None` when it may buy none.
///
/// The type comes from the same `CanBuild` answer the sidebar reads, so a
/// side without the war factory that carries it — or one at its build limit —
/// simply has no way back, exactly as a player would.
pub(super) fn yard_maker<'r>(
    rules: &'r RuleSet,
    obs: &Observation<'_>,
) -> Option<(&'r ObjectType, InternedId)> {
    super::build::class_types(rules, obs, ObjectCategory::Vehicle)
        .find(|(ty, _)| makes_a_yard(rules, ty) && yard_is_ours(rules, obs, ty))
}

/// Whether the yard a maker deploys into belongs to the house's own country.
///
/// The maker's own type is the vehicle; the country it belongs to is the yard
/// it becomes (`DeploysInto=`), and that is the type whose country list is
/// the rules' own gate. A house that bought the other side's maker would be
/// holding a vehicle it cannot deploy.
fn yard_is_ours(rules: &RuleSet, obs: &Observation<'_>, maker: &ObjectType) -> bool {
    maker
        .deploys_into
        .as_deref()
        .and_then(|name| rules.object(name))
        .is_some_and(|yard| super::build::house_owns(rules, obs, yard))
}

/// A building the yard maker's own `Prerequisite=` list asks for and the
/// house does not hold yet, when the house may build it.
///
/// Retail's `[AMCV]` wants `GAWEAP,GADEPT`: the war factory is the army's
/// anyway, but the service depot is a structure nothing else asks for, and a
/// house whose depot was never built has no way back from a lost yard — the
/// maker it needs is `CanBuild` No for good. One is bought while the yard
/// still stands, so the recovery has something to stand on.
///
/// The list is the type's own: a side whose depot is called something else is
/// answered by `CanBuild`, which is where `RequiredHouses=` and the depot's
/// own prerequisites are read.
pub(super) fn yard_maker_support<'r>(
    rules: &'r RuleSet,
    obs: &Observation<'_>,
    census: &Census,
) -> Option<(&'r ObjectType, InternedId)> {
    super::build::all_class_types(rules, obs, ObjectCategory::Vehicle)
        .filter(|(ty, _)| makes_a_yard(rules, ty) && yard_is_ours(rules, obs, ty))
        .find_map(|(maker, _)| missing_prerequisite(rules, obs, census, maker))
}

/// The first `Prerequisite=` building of `maker` the house lacks and may
/// build now.
fn missing_prerequisite<'r>(
    rules: &'r RuleSet,
    obs: &Observation<'_>,
    census: &Census,
    maker: &ObjectType,
) -> Option<(&'r ObjectType, InternedId)> {
    maker.prerequisite.iter().find_map(|entry| {
        let Prerequisite::Building(index) = *entry else {
            return None;
        };
        let want = rules.type_array_at(ObjectCategory::Building, index)?;
        let type_id = obs.type_id(&want.id)?;
        if census.type_count(type_id) > 0 {
            return None;
        }
        (obs.can_build(rules, want, true) != CanBuild::No).then_some((want, type_id))
    })
}

#[cfg(all(test, tests_sim_p_s))]
mod tests {
    use super::*;

    #[test]
    fn an_mcv_does_not_unfold_on_an_arbitrary_roadside_cell() {
        assert!(!at_selected_site((20, 20), None));
        assert!(!at_selected_site((20, 20), Some((41, 42))));
        assert!(at_selected_site((40, 41), Some((41, 42))));
    }
    use crate::sim::arena_fixture;
    use crate::sim::strategy::assist::AssistBrain;
    use crate::sim::world::Simulation;

    /// A maker whose deploy footprint is obstructed walks to ground it can use.
    ///
    /// The wishlist's report: an MCV packs, something arrives over the
    /// footprint, and the maker never deploys again. Measured before the fix —
    /// `admits=false`, the maker still in the census, `orders=0`, only the
    // attempt counter moving — because the placement sweep's first candidate is
    /// the cell the maker already occupies and the branch that receives that
    /// answer issued nothing at all.
    #[test]
    fn a_maker_whose_footprint_is_blocked_walks_to_a_site_that_takes_it() {
        let Some((mut sim, rules, owner)) = arena(0xA17) else {
            return;
        };
        let maker = sim
            .spawn_object("AMCV", "Americans", 20, 20, 255, &rules)
            .expect("an MCV");
        let yard = rules.object("GACNST").expect("the yard");
        // The deploy origin is the foundation's corner, which is not the unit's
        // cell: a hostile standing on the corner is what blocks the deploy
        // without standing on the maker.
        let origin = crate::sim::world::deploy_origin((20, 20), yard);
        assert_ne!(
            origin,
            (20, 20),
            "the origin is offset for a foundation wider than two"
        );
        sim.spawn_object(
            "E1",
            "Russians",
            origin.0.max(0) as u16,
            origin.1.max(0) as u16,
            255,
            &rules,
        )
        .expect("a hostile on the footprint");

        let mut brain = AssistBrain::new(0xA17);
        brain.census.refresh(
            &crate::rules::unit_role::RoleTable::build(&rules),
            &rules,
            &Observation::new(&sim, owner),
        );
        let obs = Observation::new(&sim, owner);
        assert!(
            !obs.deploy_site_admits(&rules, maker, yard),
            "the footprint is obstructed"
        );

        let mut out = Outbox::default();
        deploy_yard_maker(&mut brain, &rules, &obs, &mut out);
        assert!(
            out.orders
                .iter()
                .any(|order| matches!(order.goal, Goal::Move { cell } if cell != (20, 20))),
            "the maker is driven to a site that takes it: {:?}",
            out.orders.iter().map(|o| o.goal).collect::<Vec<_>>()
        );
        assert!(
            obs.deploy_admits_at(
                &rules,
                maker,
                yard,
                out.orders
                    .iter()
                    .find_map(|order| match order.goal {
                        Goal::Move { cell } => Some(cell),
                        _ => None,
                    })
                    .expect("a walk")
            ),
            "and the cell it is sent to is one the deploy would land on"
        );
    }

    /// A yard the house just gave a stance is left alone for the term, which is
    /// what stops the pack/deploy round trip: the maker a pack produces reads
    /// `deploy_site_admits` on the spot, so without the commitment it was
    /// deployed straight back out — the wishlist's "部署收起部署收起".
    #[test]
    fn a_yard_inside_its_term_is_not_deployed_again() {
        let Some((mut sim, rules, owner)) = arena(0xA17) else {
            return;
        };
        let maker = sim
            .spawn_object("AMCV", "Americans", 20, 20, 255, &rules)
            .expect("an MCV");
        let mut brain = AssistBrain::new(0xA17);
        brain.census.refresh(
            &crate::rules::unit_role::RoleTable::build(&rules),
            &rules,
            &Observation::new(&sim, owner),
        );
        let obs = Observation::new(&sim, owner);
        // Committed this frame, so the whole term still runs.
        brain.yard_commit.insert(maker, obs.tick());
        let mut out = Outbox::default();
        deploy_yard_maker(&mut brain, &rules, &obs, &mut out);
        assert!(
            !out.orders
                .iter()
                .any(|order| matches!(order.goal, Goal::Deploy)),
            "a yard inside its term is not deployed again: {:?}",
            out.orders.iter().map(|o| o.goal).collect::<Vec<_>>()
        );
        // Past the term the yard is its own decision again: the commit entry is
        // a frame long gone, so the gate reads false.
        brain.yard_commit.insert(maker, 0);
        assert!(
            !super::yard_committed(&brain, obs.tick() + super::YARD_COMMIT_FRAMES, maker),
            "a term that has run out no longer holds the yard"
        );
    }

    /// A maker with two cells that both take the deploy walks to the one the
    /// house may build on, not the nearer one the enemy covers.
    ///
    /// `deploy_admits_at` answers whether the footprint is clear, not whether
    /// the ground is worth standing on, so taking its answer alone walked the
    /// maker into the enemy's own base — the wishlist's "冲进敌人家里".
    #[test]
    fn a_maker_walks_to_buildable_ground_over_a_nearer_enemy_cell() {
        let Some((mut sim, rules, owner)) = arena(0xA17) else {
            return;
        };
        let maker = sim
            .spawn_object("AMCV", "Americans", 20, 20, 255, &rules)
            .expect("an MCV");
        let yard = rules.object("GACNST").expect("the yard");
        let origin = crate::sim::world::deploy_origin((20, 20), yard);
        sim.spawn_object(
            "E1",
            "Russians",
            origin.0.max(0) as u16,
            origin.1.max(0) as u16,
            255,
            &rules,
        )
        .expect("a hostile on the footprint");
        // An enemy building ten cells west: close enough that the inner rings
        // read `Enemy`, far enough that the outer rings do not.
        sim.spawn_object("GACNST", "Russians", 10, 10, 255, &rules)
            .expect("an enemy building");

        let mut brain = AssistBrain::new(0xA17);
        brain.census.refresh(
            &crate::rules::unit_role::RoleTable::build(&rules),
            &rules,
            &Observation::new(&sim, owner),
        );
        let obs = Observation::new(&sim, owner);
        assert!(
            !obs.deploy_site_admits(&rules, maker, yard),
            "the maker's own cell is obstructed, so the site search runs"
        );
        let space = super::super::terrain::Space::read(
            &obs,
            &rules,
            &brain.census.home_cells,
            brain.census.base_cell,
        );
        // Isolate the old terrain preference from the new threat gate;
        // the hostile infantry also serves as a genuine footprint blocker.
        let terrain_only = super::super::site_risk::YardSurvey::terrain_only_for_test();
        let cell = super::nearest_deploy_site(&obs, &rules, &space, &brain.census,
            &terrain_only, maker, yard, (20, 20)).expect("a cell that takes the deploy");
        assert!(space.ground_at(cell).buildable(),
            "the maker prefers buildable ground, got {cell:?}");
        let survey = super::super::site_risk::YardSurvey::read(&rules,&obs,yard,Some(maker));
        if let Some(candidate) = super::nearest_deploy_site(&obs, &rules, &space,
            &brain.census, &survey, maker, yard, (20,20)) {
            assert!(survey.site(&brain.census,Some((20,20)),candidate));
        }
    }

    /// The maker belongs to the yard it becomes: an American house's maker
    /// deploys into the Allied yard and a Russian house's into the Soviet
    /// one, and the yard's own country list is the rules' answer.
    #[test]
    fn the_yard_maker_is_the_house_s_own_country_s() {
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
        let allied = rules.object("AMCV").expect("the Allied maker");
        let soviet = rules.object("SMCV").expect("the Soviet maker");

        let obs = Observation::new(&sim, american);
        assert!(
            yard_is_ours(&rules, &obs, allied),
            "the Allied maker deploys into the Allied yard"
        );
        assert!(
            !yard_is_ours(&rules, &obs, soviet),
            "and the Soviet maker does not"
        );

        let obs = Observation::new(&sim, russian);
        assert!(
            yard_is_ours(&rules, &obs, soviet),
            "the Soviet maker deploys into the Soviet yard"
        );
        assert!(
            !yard_is_ours(&rules, &obs, allied),
            "and the Allied maker does not"
        );
    }
    /// The flat arena with the computer `Russians` and the human `Americans`.
    fn arena(seed: u64) -> Option<(Simulation, RuleSet, InternedId)> {
        let rules = crate::rules::retail_ini_fixture::retail_unlayered_rules()?;
        let mut sim = arena_fixture::rival_arena_known(&rules, seed);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let owner = sim.interner.intern("Americans");
        Some((sim, rules, owner))
    }

    /// One maker, bought while the yard stands, and only then.
    ///
    /// The gate used to be "no yard and no maker", which is the way back from a
    /// loss and nothing else: a house that had never lost its base never bought
    /// a second one, and the whole map's ore past its own field was somebody
    /// else's.
    #[test]
    fn a_yard_maker_is_bought_for_a_second_base_once_the_income_holds() {
        let Some((mut sim, rules, owner)) = arena(0xA17) else {
            return;
        };
        let _ = sim.spawn_object("GACNST", "Americans", 16, 16, 255, &rules);
        let _ = sim.spawn_object("GAREFN", "Americans", 18, 16, 255, &rules);
        let _ = sim.spawn_object("CMIN", "Americans", 20, 16, 255, &rules);
        let _ = sim.spawn_object("CMIN", "Americans", 20, 18, 255, &rules);
        sim.houses
            .get_mut(&owner)
            .expect("the house")
            .economy_mut()
            .set_credits_for_test(10_000);

        let mut brain = AssistBrain::new(0xA17);
        {
            let obs = Observation::new(&sim, owner);
            brain.rebuild_census(&rules, &obs);
            let budget = super::super::plan::budget(&rules, &obs, &brain.census, None);
            assert!(
                wants_a_maker(&mut brain, &rules, &obs, &budget),
                "a house with a yard, its trucks and a full wallet wants a second one"
            );
        }

        // A second yard is enough: the sweep and the walls have one more base
        // to keep tidy than they can while the first is still filling in.
        let _ = sim.spawn_object("GACNST", "Americans", 24, 16, 255, &rules);
        {
            let obs = Observation::new(&sim, owner);
            brain.census = Default::default();
            brain.rebuild_census(&rules, &obs);
            let budget = super::super::plan::budget(&rules, &obs, &brain.census, None);
            assert!(
                !wants_a_maker(&mut brain, &rules, &obs, &budget),
                "one expansion at a time"
            );
        }
    }

    /// A second base is not the first base with more of it: the cell has to be
    /// clear of every building the house already holds.
    #[test]
    fn a_blocked_mcv_site_expires_and_allows_a_new_plot() {
        assert!(retry_blocked_site(1239, 1000));
        assert!(!retry_blocked_site(1240, 1000));
        assert!(MCV_REJECT_SITE_FRAMES > MCV_CLEARANCE_FRAMES);
    }

    #[test]
    fn a_second_base_is_not_inside_the_first_one() {
        let homes = [(10, 10), (12, 10)];
        assert!(
            is_a_second_base_cell(&homes, (40, 40)),
            "a cell far away is a second base's"
        );
        assert!(
            !is_a_second_base_cell(&homes, (14, 10)),
            "and a cell beside the first yard is not"
        );
    }

    /// A maker first seen away from the base is a captured (or crated) one, and
    /// it comes home instead of deploying where it stands.
    ///
    /// The second-base search accepts the cell it was captured on — away from
    /// home, walkable, a yard fits — so without this the maker deployed beside
    /// the enemy's own yard, after walking between two answers for a while.
    #[test]
    fn a_captured_maker_comes_home_instead_of_deploying_where_it_stands() {
        let Some((mut sim, rules, owner)) = arena(0xA17) else {
            return;
        };
        let _ = sim.spawn_object("GACNST", "Americans", 16, 16, 255, &rules);
        // Far enough that it is not "at home", on ground a yard could take.
        let maker = sim
            .spawn_object("AMCV", "Americans", 24, 12, 255, &rules)
            .expect("a captured maker");

        let obs = Observation::new(&sim, owner);
        let mut brain = AssistBrain::new(0xA17);
        brain.rebuild_census(&rules, &obs);
        let mut out = Outbox::default();
        deploy_yard_maker(&mut brain, &rules, &obs, &mut out);

        assert!(
            brain.retrieve_makers.contains(&maker),
            "a maker first seen away from the base has one errand: coming home"
        );
        assert!(
            out.orders
                .iter()
                .any(|order| order.actor == maker && matches!(order.goal, Goal::Move { .. })),
            "it is walked home: {:?}",
            out.orders
        );
        assert!(
            !out.orders
                .iter()
                .any(|order| order.actor == maker && matches!(order.goal, Goal::Deploy)),
            "and it does not deploy where it was captured: {:?}",
            out.orders
        );
    }

    /// A foreign MCV that has already reached the main base is in the
    /// *deployment* stage permanently, even if its next clear foundation
    /// requires walking outside the six-cell recall radius. Earlier sweeps
    /// re-added it to retrieval on each tick, making it shuttle endlessly.
    #[test]
    fn a_homed_foreign_mcv_does_not_reenter_retrieval_outside_six_cells() {
        let Some((mut sim, rules, owner)) = arena(0xF034) else {
            return;
        };
        sim.spawn_object("GACNST", "Americans", 16, 16, 255, &rules)
            .expect("home construction yard");
        let maker = sim.spawn_object("SMCV", "Americans", 28, 16, 255, &rules)
            .expect("foreign MCV with a deployment job");
        let mut brain = AssistBrain::new(0xF034);
        brain.foreign_ready_makers.insert(maker);
        brain.retrieve_makers.insert(maker);
        let obs = Observation::new(&sim, owner);
        brain.rebuild_census(&rules, &obs);
        assert!(brain.census.base_cell.is_some());
        assert!(brain.census.base_cell.is_some_and(|home|
            super::cell_distance(home, (28, 16)) > super::RETRIEVE_ARRIVED_CELLS));
        let mut out = Outbox::default();
        deploy_yard_maker(&mut brain, &rules, &obs, &mut out);
        assert!(brain.foreign_ready_makers.contains(&maker));
        assert!(!brain.retrieve_makers.contains(&maker),
            "the foreign tech maker is deploying, never recalled twice");
        assert!(out.orders.iter().all(|order| order.actor != maker
            || !matches!(order.goal, Goal::Stop)),
            "the homed maker must not be abandoned: {:?}", out.orders);
    }

    /// A second base goes to the ore it is meant to work, not to whichever patch
    /// happens to be first.
    ///
    /// The wishlist's "占据矿脉，在矿脉附近建立分基地". The search used to walk
    /// outwards from the house and take the first cell that took a yard, so the
    /// second base landed wherever the ring order put it and the field it was
    /// built for was on the far side of the map.
    ///
    /// The map's own overlay carries the ore, so this plants a second field away
    /// from the house and asserts the site lands beside it. Without the aim the
    /// site is the ring's first cell, which is not the field.
    #[test]
    fn a_second_base_goes_to_the_ore_it_is_built_for() {
        // A field is only "far" on a map with room for one: the shared arena is
        // 32 cells square, and a ring-12 search around a cell 48 out would be
        // off the map entirely.
        let rules = crate::rules::retail_ini_fixture::retail_unlayered_rules()
            .expect("the retail rules snapshot");
        let mut sim = crate::sim::arena_fixture::rival_arena_with_size(&rules, 0xA17, 128);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        // `map_extent` reads the fog grid, which the fixture sizes separately
        // from the session's map size; a search that cannot see the map refuses
        // every cell.
        let fog = sim.fog_mut_for_test();
        fog.set_width_for_test(128);
        fog.set_height_for_test(128);
        let owner = sim.interner.intern("Americans");
        // A patch of ore well away from the house's own ground. The overlay is
        // the map's answer for "ore stands here", so this is the same fact the
        // production reader sees in a real map.
        let field = (60u16, 60u16);
        let ore = rules
            .overlay_registry()
            .stock_flat_riparius_variant_ids()
            .expect("retail names an ore overlay")[0];
        // The arena fixture carries no overlay grid, so the map's own answer for
        // "ore stands here" is installed the way a load does.
        *sim.overlay_grid_mut_for_test() =
            Some(crate::sim::overlay_grid::OverlayGrid::new(128, 128));
        for y in field.1..field.1 + 4 {
            for x in field.0..field.0 + 4 {
                sim.overlay_grid
                    .as_mut()
                    .expect("the installed overlay grid")
                    .place_overlay(x, y, ore, 0);
            }
        }
        // The yard is what makes (12, 12) the house's own ground. No maker is
        // needed: the question is where the site search aims, and that is asked
        // of the map and the ore rather than of a unit.
        let yard = sim
            .spawn_object("GACNST", "Americans", 12, 12, 255, &rules)
            .expect("a construction yard");
        let obs = Observation::new(&sim, owner);
        let mut brain = AssistBrain::new(0xA17);
        brain.rebuild_census(&rules, &obs);
        // Model a field that has genuinely been revealed to the player, not
        // one discovered by reading the hidden overlay in the search pass.
        let mut known = std::collections::BTreeMap::new();
        for y in field.1..field.1 + 4 {
            for x in field.0..field.0 + 4 {
                known.insert((x, y), 0_u8);
            }
        }

        // The aim is the known ore, not the house's own ring.
        let aim = expansion_aim(&known, (12, 12), &[(12, 12)]);
        assert_eq!(
            aim.map(|cell| cell_distance(cell, field)),
            Some(0),
            "the aim is the far field, not the ground under the yard"
        );

        // And the search lands beside it rather than beside the house. The yard
        // type is the spawned building's own, which is what `yard_type` resolves
        // through the maker's `DeploysInto=`; read directly here so the test
        // does not need a maker on the field.
        let yard_ty = obs.entity_type(&rules, yard).expect("the yard's own type");
        let found = search_expansion_site(
            &known,
            &obs,
            (12, 12),
            &[(12, 12)],
            yard_ty,
            &|_| true,
            &crate::sim::strategy::assist::terrain::Space::read(
                &obs,
                &rules,
                &[(12, 12)],
                Some((12, 12)),
            ),
        )
        .expect("ground that takes a yard");
        assert!(
            cell_distance(found, field) <= EXPAND_ORE_RING,
            "the site is beside the field: {found:?} against {field:?}"
        );
        assert!(
            cell_distance(found, (12, 12)) >= EXPAND_MIN_CELLS,
            "and still clear of the first base: {found:?}"
        );
    }

    #[test]
    fn unknown_remote_ore_cannot_set_an_expansion_target() {
        let hidden_field = (60_u16, 60_u16);
        let known = std::collections::BTreeMap::new();
        assert_eq!(expansion_aim(&known, (10, 10), &[(10, 10)]), None,
            "a hidden deposit is not a strategic target");
        let mut seen = known;
        seen.insert(hidden_field, 4);
        assert_eq!(expansion_aim(&seen, (10, 10), &[(10, 10)]), Some(hidden_field));
        assert_eq!(expansion_aim(&seen, (10, 10), &[hidden_field]), None,
            "the next base must not target its own existing field");
    }
}

#[cfg(all(test, tests_sim_p_s))]
mod rich_ore_tests {
    use super::*;

    #[test]
    fn an_isolated_nearby_ore_tile_does_not_outvote_a_rich_central_field() {
        let mut known = std::collections::BTreeMap::new();
        known.insert((24,24),0_u8);
        for x in 60..64 {
            for y in 60..64 {
                known.insert((x,y),1_u8);
            }
        }
        let chosen = expansion_aim(&known, (12,12), &[(12,12)]).unwrap();
        assert!(cell_distance(chosen, (60,60)) < 6,
            "prefer a real resource field over the first isolated tile: {chosen:?}");
    }

    #[test]
    fn a_spurious_ore_tile_is_not_a_reason_to_buy_a_yard() {
        let mut known = std::collections::BTreeMap::new();
        known.insert((24,24),0_u8);
        assert_eq!(expansion_aim(&known,(12,12),&[(12,12)]),None);
    }
}

#[cfg(all(test, tests_sim_p_s))]
mod v14_site_commit_tests {
    use super::*;
    #[test]
    fn fleeting_obstructions_do_not_redirect_the_mcv() {
        assert!(keep_expansion_appointment(300,100,true,false));
        assert!(keep_expansion_appointment(600,100,true,true));
        assert!(!keep_expansion_appointment(600,100,true,false));
        assert!(!keep_expansion_appointment(120,100,false,true));
    }
}

#[cfg(all(test, tests_sim_p_s))]
mod v28_mcv_site_commit_tests {
    use super::*;

    #[test]
    fn a_moving_mcv_keeps_a_site_until_genuinely_stalled() {
        assert!(!maker_trip_stalled(800, 500));
        assert!(!maker_trip_stalled(3_000, 2_650));
        assert!(maker_trip_stalled(3_010, 2_650));
        assert!(!maker_trip_stalled(10, 20));
    }

    #[test]
    fn a_mcv_must_approach_its_committed_plot_to_deploy() {
        assert!(!at_selected_site((50, 50), Some((70, 70))));
        assert!(at_selected_site((69, 70), Some((70, 70))));
        assert!(!at_selected_site((50, 50), None));
    }
}
