//! The assist brain: the orders and intents a player's hands would produce,
//! for whichever house the control layer drives.
//!
//! It was called `retail` until 2026-11-20. The name meant "the original game's
//! data plus a player's legal commands", and it was accurate but misleading:
//! readers took it for a port of the retail computer AI, which it is not (see
//! below). The rename is only a name — no decision, order or threshold changed.
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

//! One place for the auto play brain's diagnostics, so the files that merely
//! read a switch do not each carry a platform read of their own.

/// Whether `name` is set to anything at all: the trace switches are read as
/// `AUTOPLAY_TRACE_PUSH=1`, `AUTOPLAY_TRACE_BUILD=1` and so on, and the
/// environment they read is the one the run was launched with. Off when the
/// variable is absent.
pub(crate) fn trace_enabled(name: &str) -> bool {
    std::env::var(name).is_ok()
}


pub(super) mod advance;
pub(super) mod asset_ops;
pub(super) mod advanced;
pub(super) mod army;
pub(super) mod avoidance;
pub(super) mod build;
pub(super) mod campaign;
mod convergence;
pub(super) mod census;
pub(super) mod composition;
pub(super) mod combinations;
pub(super) mod dodge;
pub(super) mod doctrine;
pub(super) mod economy;
pub(super) mod escort;
pub(super) mod expand;
mod frontier;
pub(super) mod kite;
pub(super) mod joint_ops;
pub(super) mod micro;
pub(super) mod mission;
pub(super) mod mobilization;
pub(super) mod navigation;
pub(super) mod orders;
pub(super) mod ore;
pub(super) mod pilots;
pub(super) mod plan;
pub(super) mod recycle;
pub(super) mod recon;
mod readiness;
mod factory_handoff;
mod site_risk;
pub(super) mod siege_counter;
pub(super) mod squads;
pub(super) mod safety;
pub(super) mod strategy;
pub(super) mod tactics;
pub(super) mod tasks;
pub(super) mod terrain;
pub(super) mod threats;
pub(super) mod tracked;
pub(super) mod unstuck;
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

/// The name the status line shows for a type, or `?` when the rules do not
/// have it.
fn kind_of(obs: &Observation<'_>, rules: &RuleSet, type_id: InternedId) -> String {
    obs.object_type(rules, type_id)
        .map_or_else(|| "?".to_string(), |ty| ty.id.as_str().to_string())
}

/// The three commonest `type/mission` groups in `units`, largest first.
///
/// A count alone cannot say why a unit is standing: the mission it carries is
/// the difference between a garrison, a rifleman parked on a finished Move,
/// and a unit whose attack order has nothing left to shoot.
fn top_groups(units: Vec<(String, crate::sim::mission::MissionId)>) -> Vec<String> {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for (kind, mission) in units {
        let mission = mission.known().map_or_else(
            || format!("raw:{}", mission.raw()),
            |mission| format!("{mission:?}"),
        );
        *counts.entry(format!("{kind}/{mission}")).or_default() += 1;
    }
    let mut counts: Vec<(usize, String)> = counts
        .into_iter()
        .map(|(name, count)| (count, name))
        .collect();
    counts.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));
    counts
        .into_iter()
        .take(3)
        .map(|(count, name)| format!("{name}:{count}"))
        .collect()
}

/// Plays one house the way a player would, through commands only.
pub struct AssistBrain {
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
    pub(super) safety: safety::Director,
    factory_delivery: factory_handoff::Director,
    /// The army's teams: what each unit is for, and what it is held to. The
    /// layer between "push there" and "walk here" — rebuilt from the ledger on
    /// its own term, so a dead unit simply is not in the next count.
    pub(super) squads: squads::Squads,
    /// Persistent priorities shared by base, factories and expansion.
    pub(super) director: strategy::Director,
    pub(super) mobilization: mobilization::Mobilization,
    /// One campaign-wide commitment: keep the fist for a decisive operation
    /// while scouts, raiders, defenders and miners work uninterrupted.
    pub(super) campaign: campaign::Campaign,
    /// V36: retained fog-safe sightings and deconflicted scout assignments.
    pub(super) recon: recon::Director,
    /// V37: durable ownership of strategic neutral / captured asset operations.
    pub(super) asset_ops: asset_ops::Director,
    /// V38: a bounded cross-squad team takes one strategic prize, without
    /// claiming the whole main fist or circumventing native Capture.
    pub(super) joint_ops: joint_ops::Director,
    /// Throttle native grinder recycling so a controlled prize has time to
    /// reach the building without entering a new order auction every tick.
    pub(super) recycle_after: u64,
    /// Engineer-to-defender escort assignments; separate from player groups
    /// and from the squads that decide who may be spared.
    pub(super) escorts: BTreeMap<u64, escort::Escort>,
    /// Per-ready-building placement mission. Keeps the same footprint during
    /// clearing and awaits L2/native feedback before trying the next one.
    pub(super) site_jobs: BTreeMap<InternedId, build::SiteJob>,
    /// A completed building must still be placed beside the outpost that requested it.
    pub(super) outpost_builds: BTreeMap<InternedId, (u16, u16)>,
    pub(super) outpost_guards: BTreeMap<u64, (u16, u16)>,
    /// An MCV that cannot deploy in its current cell probes alternative yard
    /// positions. Unlike a completed structure, this is a moving actor and
    /// does not share the ready-production site's lifecycle.
    pub(super) maker_site_attempts: BTreeMap<InternedId, u32>,
    /// Queue-time preflight failed: do not buy a structure with no legal cell,
    /// and do not rescan a dense base every frame just to discover that fact.
    pub(super) site_preflight_after: BTreeMap<InternedId, u64>,
    /// The entry of the build list the house has decided on, as its index in
    /// [`plan::STEPS`], and the frame the decision may be re-made: the house
    /// saves for what it chose instead of spending the savings on whichever
    /// entry a flickering gate answers next.
    pub(super) plan_hold: Option<(usize, u64)>,
    /// The house's `Wall=yes` BuildingType, resolved once from the rules and
    /// the house's own buildability (`[GAWALL] Prerequisite=GAPILE`). Cached
    /// because the rules hold thousands of types and the answer cannot change.
    pub(super) wall_type: Option<InternedId>,
    wall_build: Option<build::WallBuild>,
    wall_order_after: u64,
    /// The protected amount for the structure the yard has settled
    /// on. Held back from the factories: a house that decides on a lab and then
    /// spends the lab's money on one more soldier every frame is a house that
    /// never reaches the lab.
    pub(super) yard_reserve: i32,
    pub(super) research_bank: economy::InvestmentBank,
    pub(super) income_bank: economy::InvestmentBank,
    pub(super) quality_bank: economy::InvestmentBank,
    pub(super) quality_reserve: i32,
    /// The buildings whose wrench the house has switched on, and the frame
    /// before which it will not switch it again. The click is a toggle, so a
    /// second one turns the repair off — and a click that was refused has to be
    /// asked again, which a memory with no term could never do.
    pub(super) repairing: BTreeMap<u64, u64>,
    /// The buildings the house has seen standing whole. A structure still going
    /// up is at a fraction of its strength by definition, and repairing it would
    /// spend credits on a build that is already paid for.
    pub(super) repaired: BTreeSet<u64>,
    /// Engineer repairs of occupied structures consume the engineer. Do not
    /// buy/dispatch a second while the first is walking to that same target.
    pub(super) engineer_repair_after: BTreeMap<u64, u64>,
    pub(super) campaign_engineer_after: u64,
    /// Repair receipt candidates: actual HP is rechecked, not inferred from
    /// the engineer disappearing. (actor, HP before request, deadline).
    pub(super) campaign_repairs: BTreeMap<u64,(u64,i32,u64)>,
    pub(super) campaign_restored_cover: BTreeMap<u64,u64>,
    /// site -> (reserved occupier, safe entrance post, hard deadline)
    pub(super) cover_detail: BTreeMap<u64,(u64,(u16,u16),u64)>,
    pub(super) crate_retry_after: BTreeMap<(u64,(u16,u16)),u64>,
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
    /// Chronosphere source and destination share one recoverable two-click mission.
    pub(super) chrono_trip: Option<orders::ChronoTrip>,
    /// At most two cheap mobile units may leave their routine for visible
    /// distant crates. A durable claim stops two units chasing the same box.
    pub(super) crate_claims: BTreeMap<u64, ((u16,u16), u64)>,
    /// Courier -> (safe regroup cell, deadline). Once a box disappears, the
    /// collector is recalled instead of being dispatched alone at enemy HQ.
    pub(super) crate_returns: BTreeMap<u64, ((u16,u16), u64)>,
    /// Multi-house attacks persist on one *visible* opponent until its known
    /// buildings disappear. Nearby hostile forward bases still outrank this.
    pub(super) campaign_house: Option<InternedId>,
    /// Last *visible* crate sightings, including those now under fog. Never
    /// populate this from hidden global crate slots.
    pub(super) crate_sightings: BTreeMap<(u16, u16), u64>,
    /// A bounded preparation window against the visible defensive gun line.
    pub(super) siege_wait: Option<(u64, u64)>,
    /// Target and start tick of a bounded local reinforcement assembly.
    /// Kept apart from the artillery window: a tank column can be scattered
    /// even without a fixed gunline, and neither timer may reset the other.
    pub(super) assault_wait: Option<(u64, u64)>,
    /// Reach of the gun line currently preventing this push, in map cells.
    pub(super) siege_reach: Option<i32>,
    /// A confirmed spy power operation persists across target re-evaluation,
    /// including the moment the tower shuts down and the siege gate disappears.
    pub(super) blackout: Option<advance::BlackoutOperation>,
    /// Reissuing a retreat every tick restarts native Move; each withdrawal
    /// attempt gets an observable travel interval before it can be retried.
    pub(super) blackout_withdraw_after: BTreeMap<u64, u64>,
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
    /// Repeated detour attempts that cannot reach a cell outside *all* known
    /// hostile guns. Keyed by actor: (objective, first blocked tick).
    pub(super) avoid_deadends: BTreeMap<u64, ((u16, u16), u64)>,
    /// After abandoning an impossible route, keep the push from sending the
    /// same actor straight back into that crossfire for a short interval.
    pub(super) avoid_retreat_until: BTreeMap<u64, u64>,
    /// The cell the second base's maker was last sent to, and the frame the
    /// search that found it ran.    ///
    /// The search is a walk over the map's ground asking the deploy's own
    /// question cell by cell, and the map is big: the answer is kept between
    /// probes instead of being recomputed every frame the maker stands still.
    pub(super) expansion_probe: Option<(u64, Option<(u16, u16)>)>,
    /// A temporary infantry obstruction must not pin a maker forever to one
    /// selected plot. After clearing fails, try another observed legal site.
    pub(super) blocked_expansion_sites: BTreeMap<(u16,u16), u64>,
    pub(super) maker_obstructed: BTreeMap<u64, ((u16,u16), u64)>,
    pub(super) maker_risk_retry: BTreeMap<u64, u64>,
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
    /// Successful, *observed* captures and their acquisition frame. These
    /// structures need real guards, not just a Capture order.
    pub(super) captured_assets: BTreeMap<u64, u64>,
    /// Defender actor -> captured asset being protected.
    pub(super) asset_guards: BTreeMap<u64, u64>,
    pub(super) asset_guard_posts: BTreeMap<u64,((u16,u16),(u16,u16),u64)>,
    pub(super) asset_guard_retry: BTreeMap<u64,u64>,
    /// Why the push did not leave this frame, as the gate that refused it:
    /// `push` when it did. Read by the forty-second census line — the question
    /// "why is the army standing in its yard" is otherwise unanswerable from a
    /// log that only counts what the house has.
    pub(super) push_gate: &'static str,
    /// The frame the army last came home, and why: `(frame, gate)`. Read by the
    /// commit window in [`advance`], so a gate that flips for one frame does not
    /// re-task the whole army. Measured on Death.mmx seed 24301: `push` for ten
    /// samples, then `outgunned` for eight, then `push`, then `outgunned` — the
    /// army walked out and back twice without either being a decision.
    pub(super) push_rest: (u64, &'static str),
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
    /// Foreign makers that have already reached home. A nearby alternate
    /// foundation can lie outside the recall radius, but must not restart a
    /// trip home after the maker begins its deployment journey.
    pub(super) foreign_ready_makers: std::collections::BTreeSet<u64>,
    /// Makers already seen, so the first sighting is the one that decides where
    /// a maker came from: a house-made one appears at its own factory, a
    /// captured one appears in the enemy's base.
    pub(super) seen_makers: std::collections::BTreeSet<u64>,
    /// Yards the house has already given a stance, by unit: the frame it was last
    /// told to pack or to deploy. Both halves of that one decision read it, so a
    /// captured yard is not packed home and deployed out again a frame later.
    pub(super) yard_commit: std::collections::BTreeMap<u64, u64>,
    /// Infiltration classes the house has already unlocked: a barracks, a war
    /// factory, a lab. One of each is the whole unlock, and a second spy buys
    /// nothing.
    pub(super) infiltrated: std::collections::BTreeSet<u8>,
    /// Which infiltration class each spy already sent was walking to, by unit.
    /// The unit is consumed on arrival, so its disappearance is the receipt.
    pub(super) spy_errand: std::collections::BTreeMap<u64, u8>,
    /// Stable last-seen sites for infiltration reconnaissance (not a direct
    /// target list). A scout sighting remains useful after the shroud closes;
    /// remove it when the cell is visible and the building is gone.
    pub(super) spy_sites: BTreeMap<(u16, u16), u64>,
    /// Agent -> (attempted building, last known cell, issued frame). A missing
    /// agent is not proof of successful infiltration, but it does release the
    /// target to be attempted again after a short recon/recruit window.
    pub(super) spy_attempts: BTreeMap<u64, (u64, (u16, u16), u64)>,
    pub(super) spy_target_retry: BTreeMap<u64, u64>,
    /// A specialist that observed a newly armed route temporarily rejects the
    /// exact dangerous objective. Never retry it on the next frame unchanged.
    pub(super) specialist_risk_after: BTreeMap<(u64, u64), u64>,
    /// Last *attempt* by effect category, not a claim of success. Repeated
    /// money/power attempts should not crowd out an available laboratory or
    /// production-factory infiltration when the route is equally safe.
    pub(super) spy_kind_recent: BTreeMap<u8, u64>,
    pub(super) spy_still: BTreeMap<u64, ((u16, u16), u64)>,
    /// Agent -> (uniform specimen id, next legal attempt frame). An L3 request
    /// does not prove the native shot fired, so retries are bounded and logged.
    pub(super) spy_uniform_retry: BTreeMap<u64, (u64, u64)>,
    /// One repair IFV command per short interval: re-aiming every tick
    /// cancels its Guard healer scan just as movement cancels weapon fire.
    pub(super) repair_ifv_after: BTreeMap<u64, u64>,
    /// A free Gunner chassis reserved for a mechanic rendezvous. The normal
    /// assault must not send the empty IFV away before the engineer boards.
    pub(super) repair_ifv_pending: BTreeMap<u64, u64>,
    /// One bounded engineer/spy ferry, whose carrier cannot be borrowed by
    /// the generic troop transport to rush an enemy base.
    pub(super) specialist_shuttles: BTreeMap<u64, orders::SpecialistShuttle>,
    pub(super) ferry_retry:BTreeMap<u64,u64>,
    pub(super) ferry_buy_after:[u64;3],
    pub(super) naval_demand:bool,
    pub(super) naval_demand_at:Option<u64>,
    /// Spy purchases are independent of engineer captures; one infiltration
    /// expedition must not be starved by unsuccessful engineer rushes.
    pub(super) next_spy_purchase: u64,
    /// Local emergency unloading is a cooldown, not a permanent garrison ban.
    pub(super) garrison_release_after: BTreeMap<u64, u64>,
    pub(super) next_garrison_purchase_scan:u64,
    pub(super) combat_feedback:composition::CombatFeedback,
    pub(super) asset_defense_builds:BTreeMap<InternedId,u64>,
    pub(super) next_asset_defense_scan:u64,
    /// A combat carrier takes at most one reserve soldier each short window.
    pub(super) combat_board_after: u64,
    /// Siege deploy is a toggle: never send it in both directions every frame.
    pub(super) siege_toggle_after: BTreeMap<u64, u64>,
    /// The ore cells near the base and the frame they were read on.
    /// A field does not move, so the scan stands for `ore::SCAN_TERM`.
    pub(super) ore_scan: Option<ore::Survey>,
    /// Last seen ore density per cell. Updates only on currently visible
    /// cells, so depleted/growing ore behind fog is not omniscient knowledge.
    ore_known: BTreeMap<(u16, u16), u8>,
    /// Surveyed ore accessible from our economy, not raw enemy-base ore.
    pub(super) expansion_ore:BTreeMap<(u16,u16),u8>,
    pub(super) expansion_bank:economy::InvestmentBank,
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
    /// Detect blocked nav and reversible two-cell loops; intervene sparingly.
    pub(super) unstuck: unstuck::Recovery,
    /// Short-lived evasions for economically valuable harvesters.
    pub(super) miner_avoid_until: BTreeMap<u64, u64>,
    pub(super) mining_hostile_sites: BTreeMap<u64,((u16,u16),i32,u64)>,
    pub(super) miner_standby: BTreeMap<u64,(u16,u16)>,
    /// Observed attacker -> (last mining incident cell, expiry).
    pub(super) mine_incidents: BTreeMap<u64,((u16,u16),u64)>,
    /// Short local clearance assignment, not a permanent second army.
    pub(super) mine_clearance: BTreeMap<u64,(u64,u64)>,
    pub(super) mine_clearance_retry: BTreeMap<u64,u64>,
    /// Previously owned victim -> (visible controller, rescue deadline).
    pub(super) liberation_window: BTreeMap<u64,(u64,u64)>,
    pub(super) liberation_retry: BTreeMap<u64,u64>,
    /// Conventional miners on an explicit journey to a *known* remote ore
    /// seam: (destination, last observed position, last progress tick).
    pub(super) miner_journeys: BTreeMap<u64, ((u16,u16), (u16,u16), u64)>,
    /// Harvest mission can remain assigned with no reachable local ore.
    /// Record a cargo-empty, stationary observation before re-dispatching.
    pub(super) miner_idle_since: BTreeMap<u64, ((u16,u16), u64)>,
    pub(super) miner_rejected: BTreeMap<u64, ((u16,u16), u64)>,
    /// Mobile Enslaves= refineries: sticky deployment target and progress.
    pub(super) slave_sites: BTreeMap<u64, ((u16,u16), (u16,u16), u64)>,
    /// A failed deployment cell is not retried on the next brain tick.
    pub(super) slave_rejected: BTreeMap<(u16,u16), u64>,
    /// Native building pack attempts must not oscillate with deploys.
    pub(super) slave_pack_after: u64,
    /// Each MCV keeps the specific mineral-side deployment plot until it
    /// arrives, becomes unreachable, or demonstrably stops making progress.
    pub(super) maker_journeys: BTreeMap<u64, ((u16,u16), (u16,u16), u64)>,
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
    pub(super) front_review: convergence::FrontReview,
    /// When a scarce, dispersed armoured fist began waiting to assemble.
    /// Unlike a target-specific timer, this survives target switches so a
    /// drifting target choice cannot restart a perpetual 900-frame hold.
    pub(super) armour_wave_wait: Option<u64>,
    /// A coherent armoured breach against one visible powered battery.
    /// Avoid/kite/dodge do not split the cohort during its brief assault.
    pub(super) siege_charge: Option<advance::SiegeCharge>,
    pub(super) charge_retry: BTreeMap<u64,advance::ChargeRetry>,
    pub(super) charge_released: BTreeMap<u64,u64>,
    /// Per-actor withdrawal leases, not a second force commander.
    pub(super) charge_rotating: BTreeMap<u64,(u64,u64)>,
    /// A visible economic victim is locked for a bounded raid, preventing
    /// the detachment from changing miners whenever positions change.
    pub(super) harass_focus: Option<(u64, u64)>,
    /// A previously *seen* mine or unloading yard. Only a navigation hint for
    /// the raiders, never permission to Attack an entity through fog.
    pub(super) harass_site: Option<((u16, u16), u64)>,
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
    /// list. `AUTOPLAY_HONEST=1` is the default; `0` enables an omniscient
    /// comparison baseline for diagnostics.
    honest: bool,
}

impl AssistBrain {
    /// Finalize the frame's best order for each unit.
    ///
    /// The outbox replaces weaker asks only when a stronger one has already
    /// been accepted. That replacement is *not* a rejection by the world: the
    /// tracker already holds the winner, and `orders_refused` can only infer a
    /// kind from a Goal. For two Move goals that would wrongly revoke Kite or
    /// Dodge (both encode as Move), then cool the winner for 30 frames.
    ///
    /// Actual L2 admission failures are reported later by ControlBoard, and
    /// those alone call `orders_refused`.
    fn finish_orders(&mut self, out: &mut Outbox) {
        out.resolve();
    }

    /// `seed` seeds this brain's own generator. It must not be a simulation
    /// stream: the brain's draws are not part of the world's random sequence.
    pub fn new(seed: u64) -> Self {
        Self {
            rng: SimRng::new(seed),
            tracked: Tracker::new(),
            safety: safety::Director::default(),
            factory_delivery: factory_handoff::Director::default(),
            squads: squads::Squads::new(),
            director: strategy::Director::default(),
            mobilization: mobilization::Mobilization::default(),
            campaign: campaign::Campaign::default(),
            recon: recon::Director::default(),
            asset_ops: asset_ops::Director::default(),
            joint_ops: joint_ops::Director::default(),
            recycle_after: 0,
            escorts: BTreeMap::new(),
            site_jobs: BTreeMap::new(),
            outpost_builds: BTreeMap::new(),
            outpost_guards: BTreeMap::new(),
            maker_site_attempts: BTreeMap::new(),
            site_preflight_after: BTreeMap::new(),
            plan_hold: None,
            wall_type: None,
            wall_build: None,
            wall_order_after: 0,
            yard_reserve: 0,
            research_bank: economy::InvestmentBank::default(),
            income_bank: economy::InvestmentBank::default(),
            quality_bank: economy::InvestmentBank::default(),
            quality_reserve: 0,
            repairing: BTreeMap::new(),
            repaired: BTreeSet::new(),
            engineer_repair_after: BTreeMap::new(),
            campaign_engineer_after: 0,
            campaign_repairs: BTreeMap::new(),
            campaign_restored_cover: BTreeMap::new(),
            cover_detail: BTreeMap::new(),
            crate_retry_after: BTreeMap::new(),
            roles: None,
            census: Census::default(),
            super_at: BTreeMap::new(),
            chrono_trip: None,
            crate_claims: BTreeMap::new(),
            crate_returns: BTreeMap::new(),
            campaign_house: None,
            crate_sightings: BTreeMap::new(),
            siege_wait: None,
            assault_wait: None,
            siege_reach: None,
            blackout: None,
            blackout_withdraw_after: BTreeMap::new(),
            income: plan::IncomeMeter::new(),
            avoid_cursor: 0,
            avoid_deadends: BTreeMap::new(),
            avoid_retreat_until: BTreeMap::new(),
            expansion_probe: None,
            blocked_expansion_sites: BTreeMap::new(),
            maker_obstructed: BTreeMap::new(),
            maker_risk_retry: BTreeMap::new(),
            engineers: composition::EngineerLedger::default(),
            capturable_last: BTreeSet::new(),
            captured_assets: BTreeMap::new(),
            asset_guards: BTreeMap::new(),
            asset_guard_posts: BTreeMap::new(),
            asset_guard_retry: BTreeMap::new(),
            push_gate: "none",
            push_rest: (0, "none"),
            sale_at: 0,
            sale_attempts: std::collections::BTreeMap::new(),
            retrieve_makers: std::collections::BTreeSet::new(),
            foreign_ready_makers: std::collections::BTreeSet::new(),
            seen_makers: std::collections::BTreeSet::new(),
            yard_commit: std::collections::BTreeMap::new(),
            infiltrated: std::collections::BTreeSet::new(),
            spy_errand: std::collections::BTreeMap::new(),
            spy_sites: BTreeMap::new(),
            spy_attempts: BTreeMap::new(),
            spy_target_retry: BTreeMap::new(),
            specialist_risk_after: BTreeMap::new(),
            spy_kind_recent: BTreeMap::new(),
            spy_still: BTreeMap::new(),
            spy_uniform_retry: BTreeMap::new(),
            repair_ifv_after: BTreeMap::new(),
            repair_ifv_pending: BTreeMap::new(),
            specialist_shuttles: BTreeMap::new(),
            ferry_retry:BTreeMap::new(),
            ferry_buy_after:[0;3],
            naval_demand:false,
            naval_demand_at:None,
            next_spy_purchase: 0,
            garrison_release_after: BTreeMap::new(),
            next_garrison_purchase_scan:0,
            combat_feedback:composition::CombatFeedback::default(),
            asset_defense_builds:BTreeMap::new(),
            next_asset_defense_scan:0,
            combat_board_after: 0,
            siege_toggle_after: BTreeMap::new(),
            ore_scan: None,
            ore_known: BTreeMap::new(),
            expansion_ore:BTreeMap::new(),
            expansion_bank:economy::InvestmentBank::default(),
            push_target: None,
            issued: [0; 5],
            last_tick: 0,
            unstuck: unstuck::Recovery::default(),
            miner_avoid_until: BTreeMap::new(),
            mining_hostile_sites: BTreeMap::new(),
            miner_standby: BTreeMap::new(),
            mine_incidents: BTreeMap::new(),
            mine_clearance: BTreeMap::new(),
            mine_clearance_retry: BTreeMap::new(),
            liberation_window: BTreeMap::new(),
            liberation_retry: BTreeMap::new(),
            miner_journeys: BTreeMap::new(),
            miner_idle_since: BTreeMap::new(),
            miner_rejected: BTreeMap::new(),
            slave_sites: BTreeMap::new(),
            slave_rejected: BTreeMap::new(),
            slave_pack_after: 0,
            maker_journeys: BTreeMap::new(),
            breaches: Vec::new(),
            blocked_goals: BTreeMap::new(),
            push_progress: (0, usize::MAX),
            front_review: convergence::FrontReview::default(),
            armour_wave_wait: None,
            siege_charge: None,
            charge_retry: BTreeMap::new(),
            charge_released: BTreeMap::new(),
            charge_rotating: BTreeMap::new(),
            harass_focus: None,
            harass_site: None,
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
        let Self { census, roles, tracked, .. } = self;
        let Some(roles) = roles.as_ref() else {
            return;
        };
        census.refresh(roles, rules, obs);
        // A unit handed to a player control group no longer belongs to this
        // brain's strategic plan. Do not resurrect its old mission if the
        // player later releases it back to automation.
        for unit in census.units.iter().filter(|unit| unit.hand) {
            tracked.forget_primary(unit.entity_id);
        }
        // Where the army gathers is beside the base, not inside it: the anchor
        // is the yard's own cell and the yard has no lane into it.
        census.rally = census
            .base_cell
            .and_then(|anchor| census::rally_near(obs, anchor, &census.home_cells));
    }

    /// Survey the known ore once per term and whenever an unloading bay is
    /// gained/lost. The same snapshot drives purchases and miner dispatch;
    /// two independently refreshed ore scans were how production paid for a
    /// field that miners had already given up on.
    fn refresh_ore(&mut self, rules: &RuleSet, obs: &Observation<'_>) {
        orders::observe_mining_territory(self,rules,obs);
        let mut anchors = self.census.refinery_cells.clone();
        if anchors.is_empty() {
            if let Some(home) = self.census.base_cell {
                anchors.push(home);
            }
        }
        anchors.sort_unstable();
        let stale = self.ore_scan.as_ref().is_none_or(|scan| {
            scan.at.saturating_add(ore::SCAN_TERM) <= obs.tick() || scan.anchors != anchors
        });
        if stale {
            // The native overlay grid is omniscient. Update last-seen data
            // only for visible ore, removing a depleted field only when this
            // house actually sees its cell empty.
            let seen: BTreeMap<(u16, u16), u8> = obs.ore_cells(rules).into_iter()
                .filter(|cell| obs.is_cell_visible(cell.0, cell.1))
                .filter_map(|cell| obs.overlay_data_at(cell.0, cell.1).map(|stage| (cell, stage)))
                .collect();
            self.ore_known.retain(|cell, _| {
                !obs.is_cell_visible(cell.0, cell.1) || seen.contains_key(cell)
            });
            self.ore_known.extend(seen);
            // A deployed Enslaves= resource building is an economic anchor,
            // not a walker. Its movement zone cannot stand in for the real
            // mobile miner's when testing a known seam's connectivity.
            let zone = self.census.units.iter()
                .filter(|unit| unit.traits.harvester)
                .filter_map(|unit| obs.object_type(rules, unit.type_id))
                .find(|ty| ty.category == crate::rules::object_type::ObjectCategory::Vehicle)
                .or_else(|| build::harvester(rules, obs).map(|(ty, _)| ty))
                .map_or(crate::rules::locomotor_type::MovementZone::Crusher, |ty| ty.movement_zone);
            // Preserve the raw ore memory for future liberation of a field.
            // Only currently workable territory may drive economic expansion.
            let workable:BTreeMap<_,_>=self.ore_known.iter().filter(|(cell,_)|
                !orders::hostile_mining_ground(self,**cell))
                .map(|(cell,stage)|(*cell,*stage)).collect();
            let survey=ore::Survey::read(rules,obs,&self.census,anchors,zone,&workable);
            self.expansion_ore=survey.fields.iter().filter_map(|cell|
                workable.get(cell).map(|stage|(*cell,*stage))).collect();
            self.ore_scan=Some(survey);
        }
        self.census.ore_targets = self.ore_scan.as_ref().map(|scan| scan.targets);
        if trace_enabled("AUTOPLAY_TRACE_ORE") && obs.tick() % 600 == 0 {
            if let Some(scan) = self.ore_scan.as_ref() {
                log::info!("ore survey: served={} patches={} stages={} haul={} target={:?} current={}/{}",
                    scan.fields.len(), scan.patches.len(), scan.stock, scan.mean_distance,
                    scan.targets, self.census.harvesters, self.census.refineries);
            }
        }
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

impl Brain for AssistBrain {
    fn name(&self) -> &'static str {
        "retail-ai"
    }

    fn think(&mut self, rules: &RuleSet, obs: &Observation<'_>, out: &mut Outbox) {
        self.last_tick = obs.tick();
        self.tracked.expire_restore_feedback(obs.tick());
        // One pass over the house's objects and factories, before any decision
        // asks a question about them. Every step below reads this ledger rather
        // than walking the entity store again.
        self.income.sample(obs.tick(), obs.earned());
        self.rebuild_census(rules, obs);
        self.combat_feedback.observe(rules,obs,&self.census);
        factory_handoff::observe(self, obs.tick());
        self.refresh_ore(rules, obs);
        // The house's picture: what the shroud shows, plus what it remembers
        // having seen. With AUTOPLAY_HONEST set, that picture is what the
        // passes below act on; without it the census's omniscient list stays,
        // so the switch can be measured against the policy it is replacing.
        // Only the native fog's *currently visible* objects refresh memory.
        // `revealed` means explored once and leaks live enemy coordinates when
        // an object moves through a previously explored but shrouded cell.
        self.perception
            .observe(obs.tick(), obs.visible_enemy_entities(), |cell| {
                obs.is_cell_visible(cell.0, cell.1)
            });
        if self.honest {
            self.census.enemies = self.perception.enemies();
            // The counter vector, structure counts and strike site counts must
            // read the same picture as the army target selectors do.
            self.census.recount_enemies(rules, obs);
        }
        // Before anything decides: an intent the object's own mission no longer
        // shows is an intent that did not take, and the layers below must be
        // free to ask again now rather than on its term.
        // On large maps a legal multi-minute Move is not a 450-frame mistake.
        // Preserve it only when the native destination is unchanged AND the
        // actor is making observable progress; stuck walkers still expire.
        for unit in &self.census.units {
            self.tracked.observe_travel_progress(obs.tick(), unit.entity_id,
                unit.cell, obs.destination(unit.entity_id));
        }
        self.void_orders_the_world_does_not_show();
        let abandoned=self.tracked.tasks.reconcile(obs.tick(),rules,obs,&self.census);
        for (actor,job) in abandoned {
            self.tracked.cancel_for_safety(actor);
            if let Some(target)=job.target {
                self.specialist_risk_after.insert((actor,target),obs.tick().saturating_add(450));
                if job.kind==tasks::JobKind::Engineer {
                    self.asset_ops.aborted(obs.tick(),actor,target);
                    self.tracked.cancel_capture_purchase(obs.tick(),target);
                }
            }
            self.safety.abandon(actor);
            self.tracked.hold(actor,obs.tick().saturating_add(90));
        }
        orders::observe_campaign_repairs(self,rules,obs);
        readiness::refresh(self, rules, obs);
        // What each unit is *for*, before anything decides what it does: the
        // membership is rebuilt on its own term, not every frame.
        let opponents: BTreeSet<_> = obs.visible_enemy_entities()
            .filter_map(|enemy| obs.visible_enemy_house(enemy.entity_id))
            .collect();
        self.squads.regroup_focused(obs.tick(), &self.census, opponents.len() > 1);
        if obs.forces_enabled() {
            // Assets first: a stale sighting may request reconnaissance, but
            // neither the director nor a scout may click an invisible object.
            self.asset_ops.observe(rules, obs, &self.census, &self.tracked);
            self.recon.prioritize_asset(self.asset_ops.recon_request(obs));
            self.recon.observe(rules, obs, &self.census, &self.squads);
        }
        self.mobilization.observe(rules, obs, &self.census);
        let attack_members = readiness::attack_members(self);
        readiness::report(self, obs.tick());
        let signals = strategy::Signals::read(
            rules, obs, &self.census, attack_members,
            &self.expansion_ore, self.income.per_minute(),
        );
        if let Some((from, to)) = self.director.observe(obs.tick(), signals) {
            // A new strategic priority invalidates a *purchase promise*, not
            // already-paid queues or unit missions. Do not keep saving for
            // a lab or MCV after the refinery was destroyed or a raid began.
            if matches!(to, strategy::Objective::RecoverYard
                | strategy::Objective::RecoverIncome
                | strategy::Objective::HoldBase)
            {
                self.plan_hold = None;
                self.yard_reserve = 0;
                self.research_bank.interrupt(obs.tick());
                self.expansion_bank.interrupt(obs.tick());
                self.quality_bank.interrupt(obs.tick());
                self.quality_reserve = 0;
            }
            log::info!(
                "auto play: strategy {:?} -> {:?} at {} yard={} ref={} harv={} fighters={} attack={} visible_raid={} far_ore={} income={:?} fighting_points={} power={} blackout={}",
                from, to, obs.tick(), signals.yards, signals.refineries,
                signals.harvesters, signals.fighters, signals.attack_members,
                signals.raiders_visible, signals.far_ore_stock,
                signals.income_per_minute,
                signals.fighting_points, obs.power_balance(), obs.power_blackout_active(),
            );
        }
        let emergency = matches!(self.director.objective(),
            strategy::Objective::RecoverYard | strategy::Objective::RecoverIncome
                | strategy::Objective::HoldBase) || self.director.recovering_under_fire();
        if let Some((before, after)) = self.campaign.observe(
            obs.tick(), rules, obs, &self.census, &self.squads,
            signals, emergency, self.push_target.map(|(_, _, cell)| cell),
        ) {
            log::info!(
                "auto play: campaign {:?} -> {:?} route={:?} target_fist={} seen_opponents={} tick={}",
                before, after, self.campaign.route(), self.campaign.wanted(),
                obs.enemy_house_count(), obs.tick(),
            );
        }
        self.campaign.observe_preparation(obs.tick(),rules,obs,&self.census,&self.squads,signals,
            !emergency && !self.mobilization.active() && self.siege_charge.is_none()
                && self.blackout.is_none(),self.push_target.map(|(_,_,cell)|cell));
        if obs.tick() % 600 == 0 {
            log::info!("auto play: convergence tick={} economy_rest={}",obs.tick(),self.campaign.economy_resting());
            log::info!("auto play: combat_doctrine tick={}",obs.tick());
            log::info!("auto play: combat_feedback tick={}",obs.tick());
        }
        // A siege charge belongs to a committed operation. Do not prepare
        // another suicide wave while a healthy house is deliberately saving.
        if self.director.recovering_under_fire() || self.mobilization.active() || self.campaign.economy_resting() {
            self.blackout = None;
            self.blackout_withdraw_after.clear();
            self.assault_wait = None;
        }
        advance::prepare_siege_charge(self, rules, obs);
        // What the wallet can still spend, and what the house earns: the two
        // numbers every step below is gated on, so they are read once.
        let mut budget = plan::budget(rules, obs, &self.census, self.income.per_minute());
        if self.director.emergency_spending(obs) {
            budget = budget.without_reserve(obs.credits());
        } else if let Some(floor) = self.mobilization.floor() {
            // The normal reserve is intended to protect a tank purchase, not
            // to make the first tank perpetually unaffordable. Release only
            // enough of it for one native line vehicle, not the entire bank.
            budget.free = budget.free.max(budget.without_reserve(obs.credits()).free.min(floor));
        }
        // One line every forty seconds (600 frames at 15 a second), the same
        // cadence the board and the yard deploy log on: "the AI builds nothing"
        // has to be answerable from the log the client already writes.
        if obs.tick() % 600 == 0 {
            log::info!("auto play: v58_recovery_policy tick={} loss_recovery={} home_raid={} offensive_hold={} opening_wave={} refit={}",
                obs.tick(),self.director.rebuilding_force(),self.director.under_attack_now(),
                self.director.recovering_under_fire(),self.mobilization.active(),
                self.campaign.phase()==campaign::Phase::Refit);
            if self.director.emergency_spending(obs) || obs.power_balance() < 0 {
                log::info!(
                    "auto play: recovery tick={} objective={:?} rebuilding_force={} credits={} free={} committed={} power={} blackout={} lines={:?}",
                    obs.tick(), self.director.objective(), self.director.rebuilding_force(),
                    obs.credits(), budget.free, budget.committed, obs.power_balance(),
                    obs.power_blackout_active(), self.census.lines,
                );
            }
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
                .filter(|enemy| obs.is_cell_visible(enemy.cell.0, enemy.cell.1))
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
            if trace_enabled("AUTOPLAY_V56_TRACE") {
                let full: Vec<_> = types.iter().map(|(id,count)| {
                    let name=obs.object_type(rules,*id).map_or("?",|ty|ty.id.as_str());
                    format!("{name}:{count}")
                }).collect();
                let (hulls,aa)=composition::ground_stock(self,rules,obs);
                let mut repair=0usize;
                let mut garrisoned=0usize;
                let mut ready=0usize;
                let mut idle=0usize;
                let mut capture=0usize;
                for unit in &self.census.units {
                    let id=unit.entity_id;
                    if obs.active_gunner_weapon(rules,id).is_some_and(|w|w.damage<0)
                        && advanced::repair_ifv_loaded(rules,obs,&self.census,id) { repair+=1; }
                    if unit.traits.occupier && obs.inside_transport(id).is_some_and(|parent|
                        obs.owns_object(parent) && obs.entity_type(rules,parent).is_some_and(|ty|
                            ty.category==crate::rules::object_type::ObjectCategory::Building)) { garrisoned+=1; }
                    if unit.traits.engineer && unit.mission.known()==Some(crate::sim::mission::MissionType::Capture) { capture+=1; }
                    if self.census.main_available(unit) {
                        if obs.attack_target(id).is_some_and(|target|obs.is_hostile(target)
                            && obs.observed_object_cell(target).is_some() && obs.can_hurt(rules,id,target)
                            && obs.engage(rules,id,target)==crate::sim::control::observation::Engage::Ready) { ready+=1; }
                        if obs.destination(id).is_none() && obs.attack_target(id).is_none() { idle+=1; }
                    }
                }
                log::info!("auto play: v56_capabilities tick={} roster=[{}] hulls={} mobile_aa={} repair_mode={} garrisoned={} capture_mission={} ready_attack={} available_idle={}",
                    obs.tick(),full.join(","),hulls,aa,repair,garrisoned,capture,ready,idle);
            }
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
            // Claimed and still standing: the assignment is fine and the
            // execution is not landing, so name them and their intent.
            if obs.tick() % 600 == 0 {
                for unit in self
                    .census
                    .units
                    .iter()
                    .filter(|unit| unit.traits.combat && unit.orderable)
                    // A live intent is the whole subject: a unit held but with no
                    // intent is one a routine parked on its post, which is where it
                    // belongs. Logging those buried the real cases — measured on
                    // Death.mmx seed 24301, 85 lines of which 76 were parked
                    // retreats reporting `intent=None`.
                    //
                    // Nor is every intent a walk. `Guard` and `Deploy` say where the
                    // unit is, not where it is going: a guard has no destination by
                    // definition. The same run still spent 26 of its 48 remaining
                    // lines on guards before this second filter.
                    .filter(|unit| {
                        self.tracked
                            .live_intent(obs.tick(), unit.entity_id)
                            .is_some_and(|(kind, _)| {
                                !matches!(
                                    kind,
                                    tracked::TrackedKind::Guard | tracked::TrackedKind::Deploy
                                )
                            })
                    })
                    .filter(|unit| {
                        obs.destination(unit.entity_id).is_none()
                            && obs.attack_target(unit.entity_id).is_none()
                    })
                    .take(3)
                {
                    log::info!(
                        "auto play: claimed-and-stuck {} intent={:?} dest={:?} target={:?} mission={:?}",
                        unit.entity_id,
                        self.tracked.live_intent(obs.tick(), unit.entity_id),
                        obs.destination(unit.entity_id),
                        obs.attack_target(unit.entity_id),
                        unit.mission
                    );
                }
            }
            // Why the units that cannot be ordered cannot be: a limbo unit is
            // alive, owned and undamaged, so the count alone says nothing about
            // whether it is garrisoned, still inside its factory, or parked
            // waiting for an exit cell. The mission is the one field the census
            // already carries that separates those.
            let limbo_missions = top_groups(
                self.census
                    .units
                    .iter()
                    .filter(|unit| unit.traits.combat && !unit.orderable)
                    .map(|unit| (kind_of(obs, rules, unit.type_id), unit.mission))
                    .collect(),
            );
            // And the same question for the units that *can* be ordered: an
            // idle fighter is one no layer gave a destination or a target, and
            // the mission it is sitting on is the difference between a unit
            // waiting for a slot, one parked on a finished Move, and one whose
            // attack order has nothing left to shoot.
            let idle_missions = top_groups(
                self.census
                    .units
                    .iter()
                    .filter(|unit| {
                        unit.traits.combat
                            && unit.orderable
                            && obs.destination(unit.entity_id).is_none()
                            && obs.attack_target(unit.entity_id).is_none()
                    })
                    .map(|unit| (kind_of(obs, rules, unit.type_id), unit.mission))
                    .collect(),
            );
            log::info!(
                "auto play: {} unclaimed={} roster=[{}] crates={}{:?} push={} target={} at {:?} squads={:?} orders={:?} pts={}/{} income={}/min seen={}/{} limbo_missions=[{}] idle_missions=[{}] enemy_structures={}",
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
                self.census.enemies.len(),
                limbo_missions.join(","),
                idle_missions.join(","),
                self.census.enemy_structures
            );
        }
        // A disabled work domain must not create a standing Tracker intent or
        // side effects later discarded by L2. Keep the original pass order:
        // that order has been tuned against winning fixed-seed replays.
        // The fifth key only changes actor eligibility, never these switches.
        if obs.production_enabled() {
            if self.naval_demand_at.is_none_or(|at|obs.tick().saturating_sub(at)>=120) {
                self.naval_demand=composition::naval_need(self,rules,obs);
                self.naval_demand_at=Some(obs.tick());
            }
            expand::deploy_yard_maker(self, rules, obs, out);
            build::spend(self, rules, obs, &budget, out);
            // The yard's new QueueProduction is already a commitment this
            // frame. Give the independent factory lines the remaining budget.
            let yard_cost = plan::proposed_cost(rules,obs,out);
            composition::fill_lines(self, rules, obs, &budget.after_commitment(yard_cost), out);
            if obs.tick() % 600 == 0 {
                if let Some(survey)=self.ore_scan.as_ref() {
                    log::info!("auto play: v57_income_state tick={} miners={} bays={} target_miners={} target_bays={} ore={} income={} cash={} income_bank={}",
                        obs.tick(),self.census.harvesters,self.census.refineries,survey.targets.harvesters,
                        survey.targets.refineries,survey.stock,budget.income.unwrap_or(-1),obs.credits(),
                        self.income_bank.active(obs.tick()).is_some());
                }
                log::info!("auto play: v56_quality_budget tick={} research_bank={} unit_bank={}",
                    obs.tick(),self.yard_reserve,self.quality_reserve);
                log::info!("auto play: economy_budget tick={} cash={} spent={} committed={} bank={} proposed={} rest={}",
                    obs.tick(),obs.credits(),obs.spent_credits(),budget.committed,self.yard_reserve,
                    plan::proposed_cost(rules,obs,out),self.campaign.economy_resting());
            }
        }
        if obs.forces_enabled() {
            orders::managed(self, rules, obs, obs.tick(), out);
        }
        if obs.production_enabled() && !obs.has_pending_production_commands() {
            let repair_budget = budget.after_commitment(plan::proposed_cost(rules,obs,out))
                .reserving(if self.director.under_attack_now() { 0 }
                    else { self.yard_reserve.saturating_add(self.quality_reserve) });
            orders::repairs(self, rules, obs, &repair_budget, out);
        }
        if obs.abilities_enabled() {
            orders::supers(self, rules, obs, out);
        }
        if obs.forces_enabled() {
            dodge::evacuate_superweapons(self,rules,obs,out);
            orders::bridges(self, rules, obs, out);
            orders::specialist_transports(self, rules, obs, out);
            // Boarding and engineer repairs reserve only a spare specialist.
            // Run them before generic capture selection, which otherwise
            // sends every engineer to the first visible neutral building.
            // Recycle only already controlled, low-operational-value prizes;
            // claim their long trip before generic combat/transport steps.
            recycle::run(self, rules, obs, out);
            advanced::free_repair_ifv_seat(self, rules, obs, out);
            advanced::combat_carriers(self, rules, obs, out);
            orders::supervise_specialists(self, rules, obs, out);
            orders::resume_specialist_jobs(self, rules, obs, out);
            // A single spare defender may establish an observation/security
            // post before the engineer's actual Capture is considered safe.
            asset_ops::Director::muster(self, rules, obs, out);
            orders::captures(self, rules, obs, out);
            orders::repair_garrisons(self,rules,obs,out);
            orders::protect_engineer_prizes(self, rules, obs, out);
        }
        if obs.production_enabled() {
            orders::pack_captured_yard(self, rules, obs, out);
        }
        if obs.forces_enabled() {
            // A stationary guard blocking a requested protective ring is
            // asked to take one safe step; active workers and assigned units
            // are left under their original task manager.
            if self.wall_type.is_some() && !self.director.emergency_spending(obs) && obs.tick() % 45 == 0
                && self.census.refineries > 0 && self.census.harvesters > 0
                && self.census.units.len() < 220
            {
                walls::clear_the_ring(self, rules, obs, out, obs.tick());
            }
            advanced::avoid_invulnerable_assault(self, rules, obs, out);
            advanced::attempt_liberation(self,rules,obs,out);
            advanced::clear_mine_raids(self,rules,obs,out);
            // Safe local cover is a legitimate defence during a raid.
            // Active shooters and a minimal mobile screen are protected.
            orders::garrison(self, rules, obs, out);
            advanced::protect_base(self, rules, obs, out);
            advanced::station_home_reserve(self, rules, obs, out);
            advanced::secure_outpost(self, rules, obs, out);
            advanced::secure_captured_assets(self, rules, obs, out);
            advanced::repair_ifvs(self, rules, obs, out);
            dodge::run(self, rules, obs, out);
            kite::run(self, rules, obs, out);
            orders::crates(self, rules, obs, out);
            // V40: a safe blast route can become unsafe when friends move.
            // Check in-flight bombs *before* offering a new target.
            orders::supervise_hazardous_attacks(self,rules,obs,out);
            if self.director.recovering_under_fire() || self.mobilization.active() || self.campaign.economy_resting() {
                advance::withdraw_for_recovery(self, rules, obs, out);
            } else {
                orders::suicide_errand(self, rules, obs, out);
                orders::bomb_errand(self, rules, obs, out);
            }
            composition::respond_to_threats(self,rules,obs,out);
            advance::trailing_siege(self,rules,obs,out);
            advance::local_contacts(self,rules,obs,out);
            orders::naval_operations(self,rules,obs,out);
            micro::run(self, rules, obs, out);
            avoidance::run(self, rules, obs, out);
            // A capture can borrow one spare defender, without overruling
            // emergency micro or the player's fifth-key scope.
            escort::run(self, rules, obs, out);
            // A miner in immediate danger may run home, but only through a
            // temporary command that cannot turn into a permanent mining loop.
            orders::harvester_safety(self, rules, obs, out);
            advanced::siege_artillery(self, rules, obs, out);
            // Native navigation can remain blocked after a legal Move. Detect
            // actual lack of progress before borrowing a unit for a side step.
            unstuck::run(self, rules, obs, out);
            // Tactical passes have first refusal. Restore a suspended primary
            // only when the native mission has settled and no fresher tactical
            // or specialist order already owns this unit's frame.
            mission::resume(self, rules, obs, out);
            // Defensive long-range fire stays behind the local screening
            // squad; the assault's own support formation runs in advance.
            advance::fire_support::defend(self, rules, obs, out);
            // Specialist passes may have claimed new actors since the
            // strategic sample. Refresh before the assault's final roster.
            readiness::refresh(self, rules, obs);
            siege_counter::air_strike(self, rules, obs, out);
            siege_counter::standoff_strike(self, rules, obs, out);
            // One small front + fire-support detachment may protect a
            // currently observed strategic prize. Ordinary advance excludes
            // its members until release, even when no new order is needed.
            joint_ops::Director::run(self, rules, obs, out);
            let advancing=advance::advance(self,rules,obs,out);
            advance::campaign_cleanup(self,rules,obs,out);
            recon::Director::sweep_idle(self,rules,obs,out);
            micro::idle_miner_pressure(self,rules,obs,out);
            if !advancing {
                orders::forces(self, rules, obs, out);
            }
            orders::slave_miners(self, rules, obs, out);
            orders::harvesters(self, rules, obs, out);
        }
        if obs.production_enabled() {
            dodge::sell_doomed_strike_building(self,rules,obs,out);
            orders::sell_hard_to_hold(self, rules, obs, out);
        }
        if obs.forces_enabled() {
            orders::transports(self, rules, obs, out);
            orders::deploy(self, rules, obs, out);
        }
        // Every pass has had its say: the strongest request for each actor
        // survives. Losing requests are not L2 refusals: another accepted
        // request owns that actor, and reporting the loser by Goal alone would
        // revoke the winner's Tracker record (both may encode as Move).
        self.finish_orders(out);
        if obs.forces_enabled() {
            advance::finish_charge(self,rules,obs,out);
            safety::recall_orphans(self,rules,obs,out);
            safety::review(self, rules, obs, out);
        }
        // Expansion can be enabled even when ordinary force automation is off.
        site_risk::review_makers(self, rules, obs, out);
        if obs.forces_enabled() {
            orders::review_capture_prizes(self,rules,obs,out);
            advanced::review_liberation(self,rules,obs,out);
            dodge::review_superweapon_moves(self,rules,obs,out);
        }
        // Last assistant-only gate, including direct asks and attachments.
        factory_handoff::filter(self, obs.tick(), out);
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
    }

    fn intents_refused(&mut self, refused: &[crate::sim::control::intent::Intent]) {
        for intent in refused {
            match intent {
                crate::sim::control::intent::Intent::QueueProduction { type_id }
                    if self.wall_build.as_ref().is_some_and(|wall| wall.type_id == *type_id) =>
                {
                    self.wall_build = None;
                }
                crate::sim::control::intent::Intent::PlaceBuilding { type_id, cell } => {
                    if let Some(job) = self.site_jobs.get_mut(type_id) {
                        job.refused(*cell, self.last_tick);
                    }
                }
                crate::sim::control::intent::Intent::LaunchSuperWeapon { sw_type_id, .. } => {
                    // An L3 request is not an L2 acceptance. If command budget,
                    // ownership or the ability gate refuses it, do not pretend
                    // the power has been spent and suppress it for 600 frames.
                    // Retrying instantly would flood the same gate, though.
                    self.super_at.insert(*sw_type_id, self.last_tick.saturating_add(30));
                    if self.chrono_trip.is_some_and(|trip| trip.source_type == *sw_type_id) {
                        self.chrono_trip = None;
                    } else if let Some(trip) = self.chrono_trip.as_mut() {
                        if trip.warp_type == *sw_type_id {
                            trip.destination_sent_at = None;
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn orders_suppressed(&mut self, suppressed: &[(u64, Goal)]) {
        for (actor, goal) in suppressed {
            // Manual takeover of a carrier/passenger cancels its autonomous
            // ferry ticket; the fifth control-group key is authoritative.
            self.specialist_shuttles.retain(|carrier,task|
                *carrier != *actor && task.passenger != *actor);
            if let Some(kind) = self.tracked.live_kind(self.last_tick, *actor)
                .filter(|kind| tracked::kinds_for(*goal).contains(kind))
            {
                self.tracked.void(*actor, kind);
            }
            // A player scope change is a hand-off, not a native rejection.
            // Do not resurrect the old strategic mission on a later toggle.
            self.tracked.forget_primary(*actor);
            self.tracked.tasks.release(*actor);
            self.safety.forget(*actor);
        }
    }

    fn orders_refused(&mut self, refused: &[(u64, Goal)]) {
        for (actor, goal) in refused {
            self.tracked.tasks.refused(*actor, *goal);
            if let Goal::Enter { target } = *goal {
                // A refused boarding command cannot reserve a ferry (and
                // thereby withhold the engineer from captures) for 3600 ticks.
                if self.specialist_shuttles.get(&target)
                    .is_some_and(|task| task.passenger == *actor) {
                    self.specialist_shuttles.remove(&target);
                }
            }
            if self.trace == Some(*actor) {
                log::info!("trace {actor}: {goal:?} refused by the world");
            }
            // A Goal is not a unique intent kind: Move can mean a scout
            // march, a dodge, a kite or an avoidance step. Refuse only the
            // live kind that actually issued this order, not every possible
            // translation. Otherwise a rejected kite prevents an emergency
            // dodge for another 30 frames, even though dodge did not fail.
            let issued_kind = self
                .tracked
                .live_kind(self.last_tick, *actor)
                .filter(|kind| tracked::kinds_for(*goal).contains(kind));
            if let Some(kind) = issued_kind {
                // The original order was not delivered. Clear its long term,
                // but throttle this one kind against retry storms.
                self.tracked.void(*actor, kind);
                self.tracked.refuse(self.last_tick, *actor, kind);
                let restoring = self.tracked.restore_refused(self.last_tick, *actor, *goal);
                // An ordinary rejected strategic order was never accepted
                // by L2, so it must not become a future resumable primary.
                // Tactical failures (even a Kite Attack at the same target)
                // must leave the old strategic objective untouched.
                if !restoring && !kind.temporary()
                    && self.tracked.primary_goal(*actor) == Some(*goal)
                {
                    self.tracked.forget_primary(*actor);
                }
            }
        }
    }
}

#[cfg(all(test, tests_sim_p_s))]
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
        let mut brain = AssistBrain::new(0xA17);
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
        let mut brain = AssistBrain::new(0xA17);
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
        let mut brain = AssistBrain::new(0xA17);
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

    /// A failed *new* primary order is not a long-term promise. An emergency
    /// action that happens later must not revive an order L2 never accepted.
    #[test]
    fn a_rejected_primary_order_cannot_be_resumed_later() {
        let mut brain = AssistBrain::new(0xA17);
        brain.last_tick = 100;
        let goal = Goal::Move { cell: (40, 40) };
        brain.tracked.record_goal(
            100, 7, TrackedKind::Move, None, Some((40, 40)), goal,
        );
        brain.orders_refused(&[(7, goal)]);
        assert_eq!(brain.tracked.primary_goal(7), None);
        assert!(
            !brain.tracked.admits(101, 7, TrackedKind::Move, None, Some((40, 40))),
            "a native refusal still receives its ordinary cooldown"
        );
    }

    /// A failed tactical Move may have the same Goal as the primary Move. The
    /// rejected Dodge must not discard the primary that it interrupted.
    #[test]
    fn a_rejected_dodge_keeps_its_standing_primary() {
        let mut brain = AssistBrain::new(0xA17);
        brain.last_tick = 100;
        let goal = Goal::Move { cell: (40, 40) };
        brain.tracked.record_goal(
            90, 7, TrackedKind::Move, None, Some((40, 40)), goal,
        );
        brain.tracked.record_goal(
            100, 7, TrackedKind::Dodge, None, Some((40, 40)), goal,
        );
        brain.orders_refused(&[(7, goal)]);
        assert_eq!(brain.tracked.primary_goal(7), Some(goal));
        assert!(brain.tracked.pending_resume(101, 7).is_some());
    }

    /// A losing Move and a winning Kite are both `Goal::Move`: reporting the
    /// former as an L2 refusal would erase the latter and impose its cooldown.
    /// A superseded ask is not a refused command; it was settled inside L3.
    #[test]
    fn the_superseded_march_does_not_cancel_the_winning_kite() {
        let mut brain = AssistBrain::new(0xA17);
        let mut out = Outbox::default();
        brain.tracked.record(0, 7, TrackedKind::Move, None, Some((20, 20)));
        assert!(out.want(7, tracked::rank(TrackedKind::Move), Goal::Move { cell: (20, 20) }));
        brain.tracked.record(0, 7, TrackedKind::Kite, None, Some((11, 10)));
        assert!(out.want(7, tracked::rank(TrackedKind::Kite), Goal::Move { cell: (11, 10) }));

        brain.finish_orders(&mut out);
        assert_eq!(out.orders.len(), 1);
        assert_eq!(out.orders[0].goal, Goal::Move { cell: (11, 10) });
        assert!(brain.tracked.stands(1, 7, &[TrackedKind::Kite]));
        assert!(
            brain.tracked.admits(16, 7, TrackedKind::Kite, None, Some((12, 10))),
            "the winning kite is free after its own 15-frame term"
        );
    }

    /// Goal::Move has four possible origins. A rejected kite request may
    /// throttle Kite, but it must not throttle Dodge or the scout's Move.
    #[test]
    fn a_refused_kite_does_not_cool_down_unrelated_movement() {
        let mut brain = AssistBrain::new(0xA17);
        brain.last_tick = 100;
        brain.tracked.record(100, 7, TrackedKind::Kite, None, Some((10, 11)));
        brain.orders_refused(&[(7, Goal::Move { cell: (10, 11) })]);
        assert!(
            !brain.tracked.admits(101, 7, TrackedKind::Kite, None, Some((10, 11))),
            "the rejected kite is held back"
        );
        assert!(
            brain.tracked.admits(101, 7, TrackedKind::Dodge, None, Some((11, 11))),
            "a shell dodge is still possible"
        );
        assert!(
            brain.tracked.admits(101, 7, TrackedKind::Move, None, Some((12, 11))),
            "the ordinary scout walk was never rejected"
        );
    }
}
