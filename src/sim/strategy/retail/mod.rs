//! The retail brain: the orders and intents a player's hands would produce,
//! for whichever house the control layer drives.
//!
//! It is a player's brain, not the computer AI's. The house it drives is a
//! human house: `HouseClass::Update`'s AI blocks — the base plan, the
//! choosers, the team creation and the strategy tick — are gated on "no human
//! controls it" and never run for it, and this brain does not make them run.
//! So everything here is a command a player could have issued: a deploy, a
//! build, a placement, an order on one of the house's own objects.
//!
//! Three jobs, in this order each frame:
//!
//! - **Get a base started**: deploy the MCV, so the yard exists to build
//!   from, and buy another one back when the yard is gone ([`expand`]).
//! - **Spend the money**: place what the yard has finished, start the next
//!   structure, keep the factories on harvesters, then on what the house's own
//!   teams asked for (`Suggest_New_Object`'s slot) and finally on what the army
//!   is short of ([`build`], over the job weights in `rules::unit_role`). That
//!   module holds the economy; it is this brain's own, because the native one
//!   is a computer house's and comes with money the house has not earned.
//! - **Use what it has**: an engineer repairs the bridge whose hut is down at
//!   home or takes a building it may capture, a spy walks into the building
//!   worth most, a rifleman garrisons a building near the base, an army large
//!   enough is sent at the enemy's base ([`advance`]), and while it is not, an
//!   idle combat unit holds the base ([`orders`]).
//!
//! Determinism: the brain owns its generator and never draws from a
//! simulation stream, and every decision is an integer comparison over the
//! rules' arrays and the entity store in their own order.
//!
//! RESIDUALS:
//! - The push is one rule: army size, then the nearest enemy structure. It
//!   has no retreat, no regrouping and no wave timing, so a push that is
//!   losing keeps going. Each unit's actual fight is the sim's `AttackMove`
//!   and `Guard`.
//! - The detour ([`avoidance`]) is two legs of geometry, not a path search: it
//!   does not know a wall, so a corner behind one is refused and the unit walks
//!   its march instead, and with no gap between two guns it stands at the edge
//!   of the nearer one rather than finding a way through. The local fight it
//!   weighs is a sum of `Strength=`, so armour, warheads and what a turret can
//!   bring to bear are not in the comparison. Aircraft and dug-in units are
//!   left out: neither walks a route this pass could change.
//! - Who fights, measured (`autoplay-match --dump-teams`, pinned
//!   `Death.mmx`/`0x5eed`): this house never has a native `TeamClass` at all —
//!   `HouseClass::Update`'s team block is gated on the house not being human
//!   (`ai_team_creation::update_team_creation`), which is native behaviour and
//!   stays. The computer house does get teams, and their retail scripts run
//!   (action ids 54/58/5 were observed mid-match), but only about two at a
//!   time and roughly ten of its units are in them; none of its 165
//!   `AITriggerTypes` produced a completed team by tick 6000. So on this side
//!   this brain is the whole army, and no unit of ours is ever inside a team
//!   for the native script and this module to fight over.
//! - Three push variants were built, run end to end and reverted, because
//!   each lost the match the current rule wins at tick 25636: per-unit targets
//!   matched to a structure's defenders by `unit_role` weights (timeout at
//!   36000, enemy still at 4), the fist aimed from the army's own centre
//!   instead of its first fighter (timeout, enemy 4), and one target held
//!   until destroyed (timeout, enemy 11). The push is sensitive to the exact
//!   target sequence, so a change here needs a better model of *why* the
//!   current sequence works before it can be replaced. The **order** is the
//!   part that was changed since: the fist keeps one target, and each unit is
//!   told to attack it from where it stands when the structure is already
//!   inside its own weapon's reach, and to attack-move otherwise. That is not
//!   one of the three: the failure they shared was dispersion.
//! - Only one managed routine is attached: `Retreat`, for a fighting unit
//!   under a third of its strength. `AirSplit` and `SpyApproach` are not,
//!   and the reason is the same for both — a routine that is its object's
//!   only issuer would take an aircraft off the push and a spy off the
//!   [`orders`] capture step, and the attach request carries no preference to
//!   put in their place. Both need the request to carry a target before they
//!   can be engaged without regressing the push.
//! - The ledger ([`census`]) is rebuilt every frame rather than cached across
//!   frames: a cached count that lags the entity store is a decision made on
//!   a stale army. The vectors keep their capacity, so a steady army costs no
//!   allocation; the cost is one walk of the entity store and one of each
//!   house's factories per frame.
//! - The army sees the whole map: fog honesty is still the open question
//!   `docs/autoplay.md` records. So does the purchase: the enemy's composition
//!   is read past the fog, and `unit_role` reads a type's warheads rather than
//!   the armour it will actually meet, so a type's range against the enemy's
//!   and what a turret can bring to bear are not in the ranking yet.
//! - Target selection is one predicate, not the native one: another house's
//!   object this house is at war with, nearest first, and only inside the
//!   base radius.
//! - A yard rebuilt from a bought MCV is not defended differently from the
//!   first one, and the brain does not repair a damaged structure, so a raid
//!   that reaches the base can take it a second time.
//! - A hurt unit reaching a `UnitRepair=` bay is now the bay's: the retreat
//!   clicks it (Enter on the building, the player's own order) instead of
//!   walking at its cell, so the dock, the paid heal and the release are the
//!   sim's own chain. What is still ours is the judgement around it: the unit
//!   is held out of the push while it is under a third of its strength, and the
//!   bay's release walks it out on
//!   its own (`building_dock::release_contact`), so a unit healed back over the
//!   line is simply read as healthy next frame. Nothing re-forms it into a
//!   squad; the squads regroup on their own term.
//! - With no building left standing, `advance` has no base centre to measure,
//!   and reads the house as not under threat, so the push runs: an army is the
//!   only thing such a house has. What it does not do is rebuild — the maker
//!   it needs can still be refused by `CanBuild`, and the recovery chain in
//!   [`expand`] cannot run without a factory to buy it from.

pub(super) mod advance;
pub(super) mod army;
pub(super) mod avoidance;
pub(super) mod build;
pub(super) mod census;
pub(super) mod composition;
pub(super) mod dodge;
pub(super) mod expand;
pub(super) mod kite;
pub(super) mod micro;
pub(super) mod orders;
pub(super) mod ore;
pub(super) mod pilots;
pub(super) mod plan;
pub(super) mod squads;
pub(super) mod terrain;
pub(super) mod threats;
pub(super) mod tracked;
pub(super) mod walls;

use std::collections::{BTreeMap, BTreeSet};

use crate::rules::ruleset::RuleSet;
use crate::rules::unit_role::RoleTable;
use crate::sim::control::Goal;
use crate::sim::control::brain::{Brain, Outbox};
use crate::sim::control::observation::Observation;
use crate::sim::intern::InternedId;
use crate::sim::rng::SimRng;

use census::Census;
use tracked::Tracker;

/// How long a unit may hold a walk without changing cell before the brain
/// decides the walk is not going anywhere, in frames. Twelve seconds: long
/// enough that a unit waiting at a corner or a bay is not re-tasked, short
/// enough that a loop is broken before it is visible.
const STUCK_FRAMES: u64 = 180;

/// How far the house's own units are taken to see, in cells, until the
/// projection carries each type's Sight=. Eight is a rifleman's sight rounded
/// down to a cell grid.
const SIGHT_CELLS: i32 = 8;

/// A type's Sight value in whole cells: leptons divided by the 256 in a cell,
/// never below one, so a type with a short sight still sees its own cell.
fn sight_cells(leptons: i32) -> i32 {
    (leptons / 256).max(1)
}

/// Drop a walk that is not moving, once; release it if the retry does not
/// move either.
///
/// Only units on the `Move` mission are offered here. A guard, a capture, a
/// repair or a deployed unit is deliberately stationary and is not this
/// check's business — treating them as stuck is what releases an army that
/// was holding its ground. A cell that changes resets the term, so a unit
/// walking a long way is never touched.
fn sweep_stuck(
    now: u64,
    units: &[(u64, (u16, u16))],
    tracked: &mut Tracker,
    progress: &mut BTreeMap<u64, (u64, (u16, u16))>,
    stuck_retried: &mut BTreeSet<u64>,
) {
    const KINDS: &[tracked::TrackedKind] = &[
        tracked::TrackedKind::Move,
        tracked::TrackedKind::AttackMove,
        tracked::TrackedKind::Avoid,
        tracked::TrackedKind::Breach,
    ];
    let live: BTreeSet<u64> = units.iter().map(|(id, _)| *id).collect();
    progress.retain(|id, _| live.contains(id));
    stuck_retried.retain(|id| live.contains(id));
    for &(id, cell) in units {
        let Some(&(since, was)) = progress.get(&id) else {
            progress.insert(id, (now, cell));
            continue;
        };
        if was != cell {
            progress.insert(id, (now, cell));
            stuck_retried.remove(&id);
            continue;
        }
        if !tracked.stands(now, id, KINDS) {
            continue;
        }
        if now.saturating_sub(since) < STUCK_FRAMES {
            continue;
        }
        for kind in KINDS {
            tracked.void(id, *kind);
        }
        if stuck_retried.contains(&id) {
            for kind in KINDS {
                tracked.refuse(now, id, *kind);
            }
            stuck_retried.remove(&id);
            log::info!("auto play: unit {id} made no progress at {cell:?}; released");
        } else {
            stuck_retried.insert(id);
        }
        progress.insert(id, (now, cell));
    }
}

/// Squared cell distance: comparisons only, so the square root is not needed.
///
/// One owner for the three identical copies `orders`, `micro` and `squads`
/// used to carry. It orders candidates by how far away they are; it is not a
/// reach test. Reach is Chebyshev, the metric the game's own adjacency uses
/// (a unit neighbours all eight cells around it), so an "is it in range"
/// question measures with that instead — see `advance`'s own helper.
pub(super) fn squared_distance(a: (u16, u16), b: (u16, u16)) -> i32 {
    let dx = i32::from(a.0) - i32::from(b.0);
    let dy = i32::from(a.1) - i32::from(b.1);
    dx * dx + dy * dy
}

/// How long an order on a unit stands before the brain looks again is
/// [`tracked::TrackedKind::term`]'s answer now, and it differs per kind: a pose
/// takes its animation, a walk across the map takes the walk. Fifteen seconds
/// for the standing orders — long enough that a unit is not re-tasked every
/// frame, short enough that one which finished its attack and idled is sent on.

/// Plays one house the way a player would, through commands only.
pub struct RetailBrain {
    /// This brain's own generator. Nothing draws on it yet — every decision
    /// here is deterministic — but the stream stays the brain's own, never
    /// the world's.
    #[allow(dead_code)]
    pub(super) rng: SimRng,
    /// What each unit has already been told, and when it may be told again.
    ///
    /// Every order the brain issues goes through it — this is the L4 layer, and
    /// the reason a toggle cannot undo itself and a walk-in is not restarted
    /// every frame. One owner for "does it need telling", whatever asked.
    pub(super) tracked: Tracker,
    /// The army's teams: what each unit is for, and what it is held to. The
    /// layer between "push there" and "walk here" — rebuilt from the ledger on
    /// its own term, so a dead unit simply is not in the next count.
    pub(super) squads: squads::Squads,
    /// How far the placement sweep has moved for a structure waiting for a
    /// cell, by type: a cell the gate admits but the receiver refuses must
    /// not be asked for again.
    pub(super) site_attempts: BTreeMap<InternedId, u32>,
    /// Types the map has no cell for after every placement attempt. They stay
    /// in the factory, but they must not hold the rest of the queue behind
    /// them.
    pub(super) stranded_sites: BTreeSet<InternedId>,
    /// The entry of the build list the house has decided on, as its index in
    /// [`plan::STEPS`], and the frame the decision may be re-made: the house
    /// saves for what it chose instead of spending the savings on whichever
    /// entry a flickering gate answers next.
    pub(super) plan_hold: Option<(usize, u64)>,
    /// The house's `Wall=yes` BuildingType, resolved once from the rules and
    /// the house's own buildability (`[GAWALL] Prerequisite=GAPILE`). Cached
    /// because the rules hold thousands of types and the answer cannot change.
    pub(super) wall_type: Option<InternedId>,
    /// The credits the yard is still short of for the structure it has settled
    /// on. Held back from the factories: a house that decides on a lab and then
    /// spends the lab's money on one more soldier every frame is a house that
    /// never reaches the lab.
    pub(super) yard_reserve: i32,
    /// The buildings whose wrench the house has switched on, and the frame
    /// before which it will not switch it again. The click is a toggle, so a
    /// second one turns the repair off — and a click that was refused has to be
    /// asked again, which a memory with no term could never do.
    pub(super) repairing: BTreeMap<u64, u64>,
    /// The buildings the house has seen standing whole. A structure still going
    /// up is at a fraction of its strength by definition, and repairing it would
    /// spend credits on a build that is already paid for.
    pub(super) repaired: BTreeSet<u64>,
    /// What each type is for, read out of the warheads once and never
    /// changed. Built on the first frame, because the rules a scenario plays
    /// with are the ones the brain is handed there.
    pub(super) roles: Option<RoleTable>,
    /// The house's own ledger: what it has, what its lines hold, and what the
    /// enemy's field asks for. Rebuilt at the top of every `think`, and the one
    /// place a decision reads "how many of mine" from. Kept inside the module
    /// the ledger's own types are visible in, so a field cannot promise more
    /// than its type can be read with.
    census: Census,
    /// When each fired super may be tried again, by type. The charge is the
    /// real gate; this only keeps a refused launch from spending the frame
    /// budget every frame it stays lit.
    pub(super) super_at: BTreeMap<InternedId, u64>,
    /// The house's income, measured: samples of `Observation::earned` a window
    /// apart. Not a simulation stream and not the brain's generator — it is a
    /// reading of the house's own books, kept here because the brain is the
    /// only thing that asks how fast the house is earning.
    income: plan::IncomeMeter,
    /// Where the avoidance sweep starts in the ledger next frame. That pass may
    /// only look at part of the army per frame, and the ledger's order does not
    /// change between frames, so without a cursor the units at its end would
    /// never be looked at at all.
    pub(super) avoid_cursor: usize,
    /// The cell the second base's maker was last sent to, and the frame the
    /// search that found it ran.    ///
    /// The search is a walk over the map's ground asking the deploy's own
    /// question cell by cell, and the map is big: the answer is kept between
    /// probes instead of being recomputed every frame the maker stands still.
    pub(super) expansion_probe: Option<(u64, Option<(u16, u16)>)>,
    /// How many engineers and spies the house has bought, and how many of them
    /// came to nothing.
    ///
    /// A live count is not enough to decide whether to buy another. An engineer
    /// is consumed the moment it walks into a building, so "how many do I have"
    /// drops to zero after every success and asks for a replacement that the
    /// match has already been paid for twice over. Measured on the 1v7 lane in
    /// v2 (`autoplay/docs/handoff.md`): the policy bought engineers continuously
    /// and the wishlist records it as 狂造工程师拖垮经济.
    ///
    /// The counter is the house's, not the unit's: what matters is the ratio of
    /// engineers bought to buildings taken. Past [`ENGINEER_WASTE_PATIENCE`]
    /// failures per success, the house stops buying them for a while — the
    /// cooldown is the frame the last one was paid for plus the term below.
    pub(super) engineers: composition::EngineerLedger,
    /// Buildings that could be captured the last time the capture step looked.
    ///
    /// `capturable_buildings` answers "what may be taken", so a building that
    /// was on this list and is no longer on it either changed hands or was
    /// destroyed. Pairing that with "and this house owns it now" is the only
    /// signal the capture line has that an engineer's errand paid off, and the
    /// engineer's own disappearance cannot tell success from death.
    pub(super) capturable_last: BTreeSet<u64>,
    /// Why the push did not leave this frame, as the gate that refused it:
    /// `push` when it did. Read by the forty-second census line — the question
    /// "why is the army standing in its yard" is otherwise unanswerable from a
    /// log that only counts what the house has.
    pub(super) push_gate: &'static str,
    /// The frame the next sale may be made on.
    ///
    /// A building that cannot be held is sold once and is then gone, so the
    /// sale cannot repeat itself; what needs a floor is the *rate*, because a
    /// base that lost its ground can qualify a dozen buildings in one frame and
    /// a dozen refunds at once is a windfall the wishlist did not ask for.
    pub(super) sale_at: u64,
    /// Sales tried per building, and the ones written off after
    /// [`super::orders::SALE_ATTEMPTS`].
    ///
    /// The comment on `sale_at` assumes a sale that qualifies is a sale that
    /// takes. Measured, one did not: `CAFRMB` at (110, 63) was sold every
    /// `SALE_GAP_FRAMES` for a whole match and was still standing at the end —
    /// a captured type the house's own yards cannot produce, which the refund
    /// path has no answer for. Without this the step has no feedback at all and
    /// spends a sale slot every four seconds for the rest of the match.
    pub(super) sale_attempts: std::collections::BTreeMap<u64, u32>,
    /// Makers that must come home before anything else: a captured MCV, or one out
    /// of a crate. Sticky — see [`expand::deploy_yard_maker`].
    pub(super) retrieve_makers: std::collections::BTreeSet<u64>,
    /// Makers already seen, so the first sighting is the one that decides where
    /// a maker came from: a house-made one appears at its own factory, a
    /// captured one appears in the enemy's base.
    pub(super) seen_makers: std::collections::BTreeSet<u64>,
    /// Infiltration classes the house has already unlocked: a barracks, a war
    /// factory, a lab. One of each is the whole unlock, and a second spy buys
    /// nothing.
    pub(super) infiltrated: std::collections::BTreeSet<u8>,
    /// Which infiltration class each spy already sent was walking to, by unit.
    /// The unit is consumed on arrival, so its disappearance is the receipt.
    pub(super) spy_errand: std::collections::BTreeMap<u64, u8>,
    /// The ore cells near the base and the frame they were read on.
    /// A field does not move, so the scan stands for `ore::SCAN_TERM`.
    pub(super) ore_scan: Option<(u64, Vec<(u16, u16)>)>,
    /// The structure the fist committed to, and the frame the commitment may be
    /// re-made: `(entity, until)`. Seven bases make "the nearest structure" a
    /// different answer every time the army moves, and an army that re-picks
    /// every frame walks between seven bases and takes none of them.
    pub(super) push_target: Option<(u64, u64, (u16, u16))>,
    /// Orders issued so far, by kind: attack, attack-move, move, guard, other.
    /// A census that counts what the house has cannot answer "what is the army
    /// being told to do", and that is the question a stalled push asks.
    pub(super) issued: [usize; 5],
    /// The frame `think` last ran on.
    ///
    /// Refusals arrive after `think` returns, and a refused command has to be
    /// stamped with the frame it was asked on so the intent table can leave it
    /// alone for its own cool-off instead of the layer re-asking it every
    /// frame.
    last_tick: u64,
    /// Where each eligible unit was last seen, for the no-progress check.
    progress: BTreeMap<u64, (u64, (u16, u16))>,
    /// Units already re-tasked once for standing still.
    stuck_retried: BTreeSet<u64>,
    /// Cells an unarmed unit found closed, and the frame each request stands
    /// until, in `(cell, until)` order.
    ///
    /// Kept across frames because the gun that opens a gate is rarely free the
    /// frame the engineer finds it: the helper that answered this frame's
    /// request is one unit with its own order, and a request that lives only
    /// one frame is a request the engineer has to run into again — which is the
    /// "the gate takes forever to open" report. Each request expires on its own
    /// frame, so a wall nobody can open is not asked for forever.
    pub(super) breaches: Vec<((u16, u16), u64)>,
    /// Destinations the map refused, by unit: where it was told to walk and
    /// when. The refusal is the one fact the obstacle answer needs and the
    /// order gate could not keep — a walk to a cell behind a gate leaves the
    /// unit standing in front of it with no mission and no record, and every
    /// pass that reads missions sees an idle unit.
    pub(super) blocked_goals: BTreeMap<u64, ((u16, u16), u64)>,
    /// When the enemy last lost a **structure**, and how many it had then: the
    /// group-level progress reading. A push that has not taken a building in a
    /// minute is not waiting for a better moment — the moment is the problem —
    /// and the army goes anyway. Killing units does not count: a trade in the
    /// field that never reaches the base is exactly the stalemate this reads.
    pub(super) push_progress: (u64, usize),
    /// The object whose orders are traced, from AUTOPLAY_TRACE: a diagnostic
    /// switch read once at construction, never a decision input. "Why did this
    /// unit do that" is the question every play test asks, and until now the
    /// only answer was to grep four passes' debug lines and guess at the
    /// interleaving.
    pub(super) trace: Option<u64>,
    /// `AUTOPLAY_TRACE=list`: print the house's own units with their ids, so a
    /// player can pick one to trace instead of guessing at a stable id.
    trace_list: bool,
    /// What the house knows: sightings and the memories of them. Fed every
    /// frame; the enemy list it replaces is the census's, which is omniscient.
    pub(super) perception: crate::sim::control::perception::Perception,
    /// Whether the brain acts on `perception` instead of the census's enemy
    /// list, from `AUTOPLAY_HONEST`: off by default while the policy is moved
    /// onto the picture a player has.
    honest: bool,
}

impl RetailBrain {
    /// `seed` seeds this brain's own generator. It must not be a simulation
    /// stream: the brain's draws are not part of the world's random sequence.
    pub fn new(seed: u64) -> Self {
        Self {
            rng: SimRng::new(seed),
            tracked: Tracker::new(),
            squads: squads::Squads::new(),
            site_attempts: BTreeMap::new(),
            stranded_sites: BTreeSet::new(),
            plan_hold: None,
            wall_type: None,
            yard_reserve: 0,
            repairing: BTreeMap::new(),
            repaired: BTreeSet::new(),
            roles: None,
            census: Census::default(),
            super_at: BTreeMap::new(),
            income: plan::IncomeMeter::new(),
            avoid_cursor: 0,
            expansion_probe: None,
            engineers: composition::EngineerLedger::default(),
            capturable_last: BTreeSet::new(),
            push_gate: "none",
            sale_at: 0,
            sale_attempts: std::collections::BTreeMap::new(),
            retrieve_makers: std::collections::BTreeSet::new(),
            seen_makers: std::collections::BTreeSet::new(),
            infiltrated: std::collections::BTreeSet::new(),
            spy_errand: std::collections::BTreeMap::new(),
            ore_scan: None,
            push_target: None,
            issued: [0; 5],
            last_tick: 0,
            progress: BTreeMap::new(),
            stuck_retried: BTreeSet::new(),
            breaches: Vec::new(),
            blocked_goals: BTreeMap::new(),
            push_progress: (0, usize::MAX),
            trace: std::env::var("AUTOPLAY_TRACE")
                .ok()
                .and_then(|raw| raw.trim().parse::<u64>().ok()),
            trace_list: std::env::var("AUTOPLAY_TRACE")
                .ok()
                .is_some_and(|raw| raw.trim().eq_ignore_ascii_case("list")),
            perception: crate::sim::control::perception::Perception::new(),
            // On by default: the house acts on what it can see and remember.
            // AUTOPLAY_HONEST=0 restores the omniscient list for a comparison.
            honest: std::env::var("AUTOPLAY_HONEST").map_or(true, |raw| raw.trim() != "0"),
        }
    }

    /// The role table for `rules`: what each type is for, so the factories can
    /// be told what the army is short of rather than what is dearest.
    pub(super) fn roles(&mut self, rules: &RuleSet) -> &RoleTable {
        self.roles.get_or_insert_with(|| RoleTable::build(rules))
    }

    /// Rebuild this frame's ledger. The role table is built here on the first
    /// frame, because the rules a scenario plays with are the ones the brain is
    /// handed there.
    fn rebuild_census(&mut self, rules: &RuleSet, obs: &Observation<'_>) {
        if self.roles.is_none() {
            self.roles = Some(RoleTable::build(rules));
        }
        // The two halves are separate fields, so the ledger can be written
        // while the table it reads is borrowed.
        let Self { census, roles, .. } = self;
        let Some(roles) = roles.as_ref() else {
            return;
        };
        census.refresh(roles, rules, obs);
        // Where the army gathers is beside the base, not inside it: the anchor
        // is the yard's own cell and the yard has no lane into it.
        census.rally = census
            .base_cell
            .and_then(|anchor| census::rally_near(obs, anchor, &census.home_cells));
    }

    /// Drop the movement and fight intents the object's own mission no longer
    /// shows.
    ///
    /// The intent table answers "does it need telling", and it was answering
    /// from its own record alone: a unit that finished the attack it was sent
    /// on sat with a live [`TrackedKind::Attack`] for the rest of the 450-frame
    /// term — the push skips it because the intent stands, the area-guard step
    /// is skipped because the push owns the army, and the unit does nothing at
    /// all. The mission is the world's answer to whether the order took, so the
    /// two are compared once, here, before any layer reads the record.
    ///
    /// [`MissionId::effective`] is the mission the object is on *or is queued
    /// onto*: a walk that has an Attack queued behind it is already the attack's
    /// and is not voided out from under itself.
    ///
    /// Capture and Enter are deliberately not here. Both have a receiver that
    /// can break the visit off (a full depot, a building that stopped being
    /// occupiable) while leaving the unit on its feet, and neither has a way to
    /// tell that from an order that landed — voiding them would re-ask every
    /// frame, which is the storm this whole change is meant to end. Their own
    /// 300-frame term is the throttle, and `orders_refused` already drops the
    /// ones the click refuses.
    fn void_orders_the_world_does_not_show(&mut self) {
        use crate::sim::mission::{MissionId, MissionType};

        let Self {
            census, tracked, ..
        } = self;
        let attack = MissionId::from_known(MissionType::Attack);
        let move_mission = MissionId::from_known(MissionType::Move);
        for unit in &census.units {
            let mission = unit.mission;
            // The fight is over: whoever sent it, a unit no longer on Attack
            // is free to be aimed again.
            if mission != attack {
                tracked.void(unit.entity_id, tracked::TrackedKind::Attack);
                tracked.void(unit.entity_id, tracked::TrackedKind::Engage);
            }
            // The march is over. An attack-move is a march with its gun up, so
            // both are held by the same two missions.
            if mission != attack && mission != move_mission {
                tracked.void(unit.entity_id, tracked::TrackedKind::Move);
                tracked.void(unit.entity_id, tracked::TrackedKind::AttackMove);
            }
        }
    }

    /// This brain's own draw, for the native decisions it asks.
    #[allow(dead_code)]
    pub(super) fn draw(&mut self, low: i32, high: i32) -> i32 {
        self.rng.next_range_i32_inclusive(low, high)
    }
}

impl Brain for RetailBrain {
    fn name(&self) -> &'static str {
        "retail-ai"
    }

    fn think(&mut self, rules: &RuleSet, obs: &Observation<'_>, out: &mut Outbox) {
        self.last_tick = obs.tick();
        // One pass over the house's objects and factories, before any decision
        // asks a question about them. Every step below reads this ledger rather
        // than walking the entity store again.
        self.income.sample(obs.tick(), obs.earned());
        self.rebuild_census(rules, obs);
        // The house's picture: what the shroud shows, plus what it remembers
        // having seen. With AUTOPLAY_HONEST set, that picture is what the
        // passes below act on; without it the census's omniscient list stays,
        // so the switch can be measured against the policy it is replacing.
        let tick = obs.tick();
        // Visibility for the house: ground it has explored, and the ring its
        // own units stand in. The ring stands in for each type's Sight= until
        // the projection carries it; without it the switch reads an
        // unexplored map as an empty one and the house fights nobody.
        let own: Vec<((u16, u16), i32)> = self
            .census
            .units
            .iter()
            .map(|unit| {
                (
                    unit.cell,
                    obs.object_type(rules, unit.type_id)
                        .map_or(SIGHT_CELLS, |ty| sight_cells(ty.sight)),
                )
            })
            .collect();
        let sight = |cell: (u16, u16)| {
            obs.is_cell_revealed(cell.0, cell.1)
                || own.iter().any(|(at, reach)| {
                    i32::from(at.0.abs_diff(cell.0)).max(i32::from(at.1.abs_diff(cell.1))) <= *reach
                })
        };
        self.perception
            .observe(tick, self.census.enemies.iter().copied(), sight);
        if self.honest {
            self.census.enemies = self.perception.enemies();
        }
        // Before anything decides: an intent the object's own mission no longer
        // shows is an intent that did not take, and the layers below must be
        // free to ask again now rather than on its term.
        self.void_orders_the_world_does_not_show();
        // What each unit is *for*, before anything decides what it does: the
        // membership is rebuilt on its own term, not every frame.
        self.squads.regroup(obs.tick(), &self.census);
        // What the wallet can still spend, and what the house earns: the two
        // numbers every step below is gated on, so they are read once.
        let budget = plan::budget(rules, obs, &self.census, self.income.per_minute());
        // One line every forty seconds (600 frames at 15 a second), the same
        // cadence the board and the yard deploy log on: "the AI builds nothing"
        // has to be answerable from the log the client already writes.
        if obs.tick() % 600 == 0 {
            if self.trace_list {
                for unit in &self.census.units {
                    log::info!(
                        "trace list: {} {} at {:?} hp {}",
                        unit.entity_id,
                        obs.object_type(rules, unit.type_id)
                            .map_or("?", |ty| ty.id.as_str()),
                        unit.cell,
                        unit.health_permille
                    );
                }
            }
            // Shadow perception: how much of what the brain is using the
            // player could actually see. The census is omniscient today, and
            // switching it off without a reading is how a policy starts
            // fighting a world it cannot see. This measures the gap first.
            let seen = self
                .census
                .enemies
                .iter()
                .filter(|enemy| obs.is_cell_revealed(enemy.cell.0, enemy.cell.1))
                .count();
            // What the army is made of, by name: "it still only makes IFVs"
            // and "it is all one tank" are questions about the roster, and
            // the counts alone cannot answer them. The three commonest types
            // are enough to see a line that has stopped rotating.
            let mut types: Vec<(crate::sim::intern::InternedId, usize)> = self
                .census
                .by_type
                .iter()
                .map(|(type_id, count)| (*type_id, *count))
                .filter(|(_, count)| *count > 0)
                .collect();
            types.sort_by_key(|(type_id, count)| (std::cmp::Reverse(*count), *type_id));
            let roster: Vec<String> = types
                .iter()
                .take(3)
                .map(|(type_id, count)| {
                    let name = obs
                        .object_type(rules, *type_id)
                        .map_or("?", |ty| ty.id.as_str());
                    format!("{name}:{count}")
                })
                .collect();
            let crates: Vec<(u16, u16)> = obs.crate_cells(rules);
            // Unclaimed, not merely unordered: a unit with no live intent is one
            // no layer has spoken for inside the intent term. The census's own
            // `idle=` counts "no destination and no target right now", which also
            // catches units whose order is in flight, so it over-reports. This is
            // the question "has anything claimed this unit recently".
            let unclaimed = self
                .census
                .units
                .iter()
                .filter(|unit| unit.traits.combat && unit.orderable)
                .filter(|unit| !self.tracked.is_claimed(obs.tick(), unit.entity_id))
                .count();
            log::info!(
                "auto play: {} unclaimed={} roster=[{}] crates={}{:?} push={} target={} at {:?} squads={:?} orders={:?} pts={}/{} income={}/min seen={}/{}",
                self.census.describe(),
                unclaimed,
                roster.join(","),
                crates.len(),
                crates,
                self.push_gate,
                self.push_target.map_or(0, |(entity, _, _)| entity),
                self.push_target.map(|(_, _, cell)| cell),
                self.squads
                    .all()
                    .iter()
                    .map(|squad| squad.members.len())
                    .collect::<Vec<_>>(),
                self.issued,
                self.census.army_points(),
                self.push_target.map_or(0, |(_, _, cell)| {
                    self.census.enemy_points(rules, obs, cell, 18)
                }),
                budget.income.map_or(-1, |rate| rate),
                seen,
                self.census.enemies.len()
            );
        }
        expand::deploy_yard_maker(self, rules, obs, out);
        build::spend(self, rules, obs, &budget, out);
        // Units after the structures: the yard's next choice decides what the
        // house can make, and the lines fill once nothing is waiting to be
        // placed.
        composition::fill_lines(self, rules, obs, &budget, out);
        // A hurt unit is the retreat routine's before it is any order
        // step's: the attach rides the same outbox as the orders below, and
        // the control layer drops this brain's order for an object a routine
        // has just taken.
        orders::managed(self, rules, obs, obs.tick(), out);
        // A building under fire is worth less than the next one the yard makes,
        // and the wrench is the only thing that keeps it: it is a house-scoped
        // click, so it is asked before the units take their frame.
        orders::repairs(self, rules, obs, out);
        // A charged power is worth more than any single order below: it is the
        // one decision here that can take a base apart, and the button it fires
        // through is the sidebar's, not a unit's.
        orders::supers(self, rules, obs, out);
        // Specialists first, in the order a bridge at home, a spy's target and a
        // garrison matter more than another unit standing still; a unit takes
        // one order a frame, so whoever asks first keeps it.
        orders::bridges(self, rules, obs, out);
        orders::captures(self, rules, obs, out);
        orders::garrison(self, rules, obs, out);
        // A shell already in the air outranks every other unit decision: it is
        // the only one whose whole window is a handful of frames, and a unit
        // that is not there when it lands takes nothing. Everything below
        // leaves a unit this pass ordered alone.
        dodge::run(self, rules, obs, out);
        // And the other half of the same micro: a unit that out-ranges what it
        // is shooting at spends the reach only if it is not standing in the
        // enemy's own — one step out while the gun reloads, back in to fire.
        kite::run(self, rules, obs, out);
        // What each unit shoots *this second*, before the march is decided: a
        // hostile in reach outranks a walk towards a target, and this is the
        // only layer that asks.
        micro::run(self, rules, obs, out);
        // And what it walks into: a walk towards a target the unit would lose
        // against is re-aimed around it, after the fight in reach has had its
        // say and before the march decides the same unit's destination.
        avoidance::run(self, rules, obs, out);
        // A house that is pushing owns its whole army for the frame; one that
        // is not keeps holding its base.
        if !advance::advance(self, rules, obs, out) {
            orders::forces(self, rules, obs, out);
        }
        // Harvesters are not combat units and no army layer takes them: a
        // harvester that rolled out of a factory sits on its birth posture
        // until something puts it on the ore.
        orders::harvesters(self, rules, obs, out);
        // A building the house holds but cannot make, and cannot hold: sell it.
        orders::sell_hard_to_hold(self, rules, obs, out);
        // A carrier with passengers takes them where the army is going and
        // opens there: the Transport pilot dispatch.
        orders::transports(self, rules, obs, out);
        // Last of the orders that move a unit, because a crate is a bonus and
        // not a job: only a scout no step above has spoken for walks to one.
        orders::crates(self, rules, obs, out);
        // Last, because it is a pose and not a destination: a soldier takes its
        // deployed position where the orders above have left it, and the toggle
        // is issued on the edge only.
        orders::deploy(self, obs, out);
        // Every pass has had its say: keep one order per object, the strongest
        // ask, in the order the asks were made. The asks a stronger one
        // replaced are refusals like any other — the caller that made them
        // recorded an intent for an order the engine never saw, and leaving
        // that record in place is the unit waiting out a term for nothing.
        // What the army was told this frame, counted by kind: the census line
        // reports the totals, so "the army is standing in its yard" can be
        // told apart from "the army is being told to guard".
        for order in &out.orders {
            let slot = match order.goal {
                Goal::Attack { .. } => 0,
                Goal::AttackMove { .. } => 1,
                Goal::Move { .. } => 2,
                Goal::Guard => 3,
                _ => 4,
            };
            self.issued[slot] += 1;
        }
        let replaced = out.resolve();
        if !replaced.is_empty() {
            self.orders_refused(&replaced);
        }
    }

    fn orders_refused(&mut self, refused: &[(u64, Goal)]) {
        for (actor, goal) in refused {
            if self.trace == Some(*actor) {
                log::info!("trace {actor}: {goal:?} refused by the world");
            }
            for kind in tracked::kinds_for(*goal) {
                // The record goes, so the intent itself can be decided again —
                // but the refusal leaves a short term behind it. Without the
                // term the layer re-decides from the unchanged world on the
                // next frame and asks again, once per frame, for as long as the
                // refusal stands.
                self.tracked.void(*actor, *kind);
                self.tracked.refuse(self.last_tick, *actor, *kind);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use super::tracked::TrackedKind;
    use crate::sim::control::Goal;
    use crate::sim::mission::MissionType;

    /// An order the gate dropped must not silence the unit for the intent's
    /// term. The frame admits eight commands; an army of a hundred asks for
    /// more than that every frame, and the orders it loses are the ones the
    /// tracker has already promised — 450 frames of a unit standing still
    /// while its brain believes it is marching. It must not be re-asked on the
    /// very next frame either: a refusal that never changed the world produces
    /// the same order again, once per frame, which is the command storm. The
    /// refusal is therefore retried on its own short term.
    #[test]
    fn a_refused_order_frees_the_unit_to_be_told_again() {
        let mut brain = RetailBrain::new(0xA17);
        brain
            .tracked
            .record(0, 7, tracked::TrackedKind::Move, None, Some((20, 20)));
        assert!(
            !brain
                .tracked
                .admits(1, 7, tracked::TrackedKind::Move, None, Some((20, 20))),
            "the march stands until its term"
        );

        brain.orders_refused(&[(7, Goal::Move { cell: (20, 20) })]);

        assert!(
            !brain
                .tracked
                .admits(1, 7, tracked::TrackedKind::Move, None, Some((20, 20))),
            "a refusal is not retried on the very next frame"
        );
        assert!(
            brain
                .tracked
                .admits(31, 7, tracked::TrackedKind::Move, None, Some((20, 20))),
            "and is retried once the refusal has cooled off"
        );
    }

    /// A unit whose attack is over is free again. The table used to believe its
    /// own `Attack` record for the whole 450-frame term, and the push skips a
    /// unit whose intent still stands (`admits`), so a fighter that finished
    /// what it was sent at stood in the field doing nothing until the term ran
    /// out — the "it killed the target and then idled" the play test reports.
    #[test]
    fn an_attack_the_unit_has_finished_is_not_still_standing() {
        let mut brain = RetailBrain::new(0xA17);
        brain
            .tracked
            .record(0, 7, TrackedKind::Attack, Some(9), Some((10, 10)));
        brain
            .tracked
            .record(0, 8, TrackedKind::Attack, Some(9), Some((10, 10)));
        // 7 came off the attack; 8 is still on it.
        brain.census.units.push(census_unit(7, MissionType::Guard));
        brain.census.units.push(census_unit(8, MissionType::Attack));
        assert!(
            !brain
                .tracked
                .admits(1, 7, TrackedKind::Attack, Some(9), Some((10, 10))),
            "the record stands until the world says otherwise"
        );

        brain.void_orders_the_world_does_not_show();

        assert!(
            brain
                .tracked
                .admits(1, 7, TrackedKind::Attack, Some(9), Some((10, 10))),
            "a finished attack is not an order that still stands"
        );
        assert!(
            !brain
                .tracked
                .admits(1, 8, TrackedKind::Attack, Some(9), Some((10, 10))),
            "the unit still on Attack is left to its mission"
        );
    }

    /// One ledger row for the intent tests: no type, no traits, just the
    /// mission the comparison is about.
    fn census_unit(entity_id: u64, mission: MissionType) -> census::Unit {
        census::Unit {
            entity_id,
            type_id: crate::sim::intern::InternedId::default(),
            cell: (10, 10),
            health_permille: 1000,
            points: 0,
            mission: crate::sim::mission::MissionId::from_known(mission),
            orderable: true,
            hand: false,
            slot: census::Slot::Vehicle,
            tier: 0,
            traits: census::Traits::default(),
        }
    }

    /// The other half: a refusal names a goal, and only the intent that goal was
    /// recorded as is dropped. `Goal::Attack` is both the micro layer's `Engage`
    /// and the squad's `Attack`, so the refusal drops whichever one this unit
    /// actually holds and leaves everything else alone. The dropped kind is
    /// retried on the refusal's short term, not on the next frame.
    #[test]
    fn a_refused_attack_does_not_drop_an_unrelated_intent() {
        let mut brain = RetailBrain::new(0xA17);
        brain
            .tracked
            .record(0, 7, tracked::TrackedKind::Engage, Some(9), Some((5, 5)));
        brain
            .tracked
            .record(0, 8, tracked::TrackedKind::Capture, Some(9), Some((5, 5)));

        brain.orders_refused(&[(7, Goal::Attack { target: 9 })]);

        assert!(
            !brain
                .tracked
                .admits(1, 7, tracked::TrackedKind::Engage, Some(9), Some((5, 5))),
            "the refusal is not retried on the next frame"
        );
        assert!(
            brain
                .tracked
                .admits(31, 7, tracked::TrackedKind::Engage, Some(9), Some((5, 5))),
            "the engaged unit is free again once the refusal cools off"
        );
        assert!(
            !brain
                .tracked
                .admits(1, 8, tracked::TrackedKind::Capture, Some(9), Some((5, 5))),
            "another unit's capture walk is untouched"
        );
    }
}
