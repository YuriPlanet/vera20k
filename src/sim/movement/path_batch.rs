//! Batched path searches: a frame's waiting Find_Path requests search side
//! by side.
//!
//! Two requests wait. A Foot with no path queue makes a no-queue request
//! (Walk 0x75AFC5, Drive 0x4B28A3, Ship 0x6A1EF3) whenever its movement timer
//! has run out: its first request after a new destination and each one after
//! a route runs out. A Foot whose next cell holds a moving friendly
//! (Can_Enter_Cell code 2) re-paths from its code-2 ladder (Drive/Ship
//! 0x4B3607..0x4B3A94, Walk 0x0075B8A9..0x0075B9F9) whenever that timer has
//! run out. With batching on, either registers the Foot and takes the
//! timer-still-running branch instead. At the start of the next frame's live
//! object pass the registrations are searched on worker threads against the
//! world as it stands, up to a cap (below), and the Foot's request then runs
//! its Find_Path with the answer.
//!
//! Sim-scale deviation from native order (update order and
//! same-frame effects), pending side-by-side play: a Foot's path search runs
//! a frame later (more under the cap below), from the world at the start of
//! the pass rather than at its own turn, so a Foot starts along a new route
//! that much later, and its movement timer (PathDelay) is armed that much
//! later, which shifts its later requests too. A Foot that stops repeating
//! its request does not search at all: a blocked Foot whose blocker moved on
//! keeps its route; a no-queue request whose destination cleared is dropped; a
//! Drive/Ship code-2 ladder whose destination its own Find_Path redirect
//! stopped (a Chrono Miner bound for its refinery, #1672) waits, its request
//! gets no answer and Process_Movement then returns on the null destination,
//! so the NULL destination native sets in that call is not set, and the
//! redirect's NavCom stays for the queued Enter mission. Two
//! visible cases: Infantry Scatter's immediate Process (0x51D478) waits, so
//! scattered infantry leave a frame later; a Drive/Ship Foot whose track
//! ends with no path left loses that frame's movement where native's timer
//! had run out. Through these requests a Foot searches at most once a
//! frame, and only when the pass answered it: its first request that frame
//! takes the answer, or searches for itself when none fits, and any other
//! waits. That includes the requests of a refused cell's recursion (Walk's
//! admission retries, which expire the timer; Drive/Ship's Process_Movement
//! re-entries, of which the 0x4B4541 tail expires it) and a code-2 re-path
//! right after a found route, whose timer Find_Path left run out. So a Foot
//! whose next cell refuses it stands that frame with its head cleared and
//! re-paths the next, where native re-paths and usually steps at once, and a
//! Foot whose new route is blocked at its first step by a moving friendly
//! re-paths a frame later. A recursion's request registers as that
//! recursion, and the request that takes its answer resumes with the
//! recursion's arguments, as the recursion runs:
//! - A retry's recursion ([`WaitingRequest::RetryRecursion`]), Walk's
//!   (Process argument 0) and Drive/Ship's (Process_Movement(0, 0)), resumes
//!   with the retry spent: a step refused again takes the argument-0 arms,
//!   such as code 6's stop within CloseEnough of the destination (Walk
//!   0x75B725..0x75B836, Drive 0x4B3742..0x4B38A1) or Drive's code-7 stop
//!   (0x4B3C26), instead of retrying every frame.
//! - Drive/Ship's second-stage code-4/5 re-entry
//!   ([`WaitingRequest::StraightReentry`]) clears the path and passes
//!   argument 2 on with argument 3 = 1 (Drive 0x4B41B3..0x4B41F7, Ship
//!   0x6A37DF..0x6A3823), so the new route's first track runs straight
//!   (Drive 0x4B3F7D..0x4B3F93, Ship 0x6A35CC..0x6A35E2) instead of
//!   turning into the same refusal every frame.
//!
//! A recursion a capped pass leaves out stays one through the Foot's
//! registrations, as a left-out code-2 re-path keeps its urgency. The
//! request that takes the answer resumes as the recursion even when the Foot
//! was given a new destination in between, which native's same-frame
//! recursion never meets: a retry would usually reach the same arm, and the
//! new route's first track runs straight.
//! Hover's own timer-gated Find_Path is not batched.
//! The batch draws no random numbers; later draws follow the changed
//! movement. Each search reads only that frame-start world, so no result
//! depends on the thread count. Simulations start without batching; real
//! matches and sim-bench turn it on ([`Simulation::batch_path_searches`]).
//!
//! A request that heads for the Foot's own cell never waits, even when the
//! pass did not answer the Foot or the Foot searched already that frame: it
//! runs in native order, in the same call. It seldom searches at all, since
//! AStar returns no route for a goal in its start cell and height
//! (0x429BF3..0x429C0A), and its Find_Path failure is what stops the Foot:
//! PathDelay, then Stop_Driver (0x4D4044; the Unit's is 0x4D55C0), whose
//! Drive Stop_Moving clears the destination (0x4AFEA7..0x4AFEC0). Waiting
//! could strand the Foot. A Drive/Ship Foot whose track ends exactly on its
//! destination with no NavCom, as a Tank Bunker's install Force_Track does,
//! makes that request in the same Process (Drive 0x4B0647, Ship 0x69FCEE).
//! Had it waited, a Guard Foot's next Process would take the at-destination
//! stop (Drive 0x4B06D5..0x4B0772, Ship 0x69FD7F..0x69FE1C) before
//! Process_Movement repeats the request, and that stop's
//! SetDestination(NULL, 1) does nothing without a NavCom (Unit
//! 0x741A80..0x741A90). The Foot would stay moving on the spot (Drive
//! Is_Moving 0x4AFB80), so the bunker, which waits for it to stop
//! (`BuildingClass::UpdateBunker` 0x458E50 at 0x45920B), would never raise
//! its walls (#2448).
//!
//! A second sim-scale deviation (update order), pending side-by-side
//! play, caps the work: a pass answers at most [`SEARCHES_PER_PASS`]
//! registrations besides its code-2 re-paths (below), taking them in
//! ascending Foot id from where the last capped pass stopped and wrapping
//! round. The others have no answer, so
//! their requests that frame register again, as any Foot's without one do,
//! and they wait another frame, standing as they do while their timer runs.
//! The cap binds only when more Foots than that wait in one frame, as after
//! an order given to that many at once. Then the last of N waiting Foots
//! waits about N / [`SEARCHES_PER_PASS`] frames, one sweep of the turn; Foots
//! start their routes in bands of ascending id; and the cap is shared, so
//! one House's mass order delays every House's requests.
//!
//! A third sim-scale deviation (update order), pending side-by-side
//! play, bounds the work of the code-2 re-paths, which Foots a moving
//! friendly blocks ask: at urgency 1 for a way past the traffic, at urgency
//! 2 for one round every moving friendly. Each urgency's re-paths take their
//! own turn round the Foot ids, as the cap's turn does: a pass takes at most
//! [`SEARCHES_PER_PASS`] of them in ascending Foot id from the first one a
//! pass left out, wrapping round; it starts them before the other
//! registrations, the urgency-2 turn first, and once a turn's expansions
//! reach [`REPATH_STEPS_PER_PASS`] it answers none after them in that turn;
//! the one that reaches it keeps its answer. The rest have no answer, so
//! their requests that frame register again, still at their urgency, and the
//! next pass starts the turn at the first of them. Which re-paths a pass
//! answers depends on their expansions alone, not on the thread count, and
//! a Foot that asks a re-path at every pass waits at most one sweep of its
//! turn: about as many passes as the expansions of those ahead of it fill
//! budgets. A re-path's expansions are those of all its cell
//! searches, the retries after a failed corridor search included (up to
//! five; see `zone_search`). The budgets bind whenever a frame's jams
//! re-path at scale. In the match bench's jams one urgency-2 re-path routes
//! round thousands of moving friendlies, often expanding 40,000 cells, and
//! before its budget a pass of a few hundred held its frame for up to a
//! tenth of a second; there the jammed Foots find their detours later, so
//! fewer reach combat. In sim-bench's 20,000-unit march the urgency-1
//! re-paths made 42% of all expansions before their budget, and with it the
//! march makes about a quarter fewer; stuck Foots wait longer there, so its
//! passes answer about a quarter fewer registrations. A Foot a budget
//! leaves out stands where its blocker holds it, as one the cap leaves out
//! does, and one whose blocker moves on meanwhile carries on along its route
//! and asks no re-path. A re-path past its budget does not start, or stops
//! searching, once the ones before it in its turn have made the budget's
//! expansions. No-queue requests and recursions have no budget, so a mass
//! order's first routes start within the cap whatever the jams around them.
//!
//! A code-2 re-path searches at the urgency its ladder asked when the Foot
//! registered it, the frame native searched: 2 once the Foot's latched
//! blockage timer had run out, else 1. A Foot a pass leaves out keeps that
//! urgency while it registers the re-path again every frame, so a wait past
//! its blockage timer does not raise it
//! ([`Simulation::batched_code2_urgency`]). A Foot that skips a frame, or
//! registers another request, starts afresh.
//!
//! The batch runs Find_Path's own steps up to its redirect
//! ([`Simulation::find_path_target_answer`]) and the core search, without
//! their effects. A redirect retargets the Foot's NavCom, which the core
//! search reads, so a request that redirects is left to the Foot's own
//! Find_Path. That Find_Path still runs at the Foot's turn, in native order,
//! and takes the answer in place of its core search only when the request
//! and the Foot's own search inputs are the ones the answer was computed for
//! ([`FootSearchKey`]); otherwise it searches as usual.
//!
//! The batch searches before Mark0 lifts the Foot off its cell; see
//! [`MoverLift::Emulated`] for what the search emulates and drops. The
//! blocker plane needs an overlay grid, through which movers reach it by
//! their Foot+55C counters, which Mark0 leaves alone. Lookups off the map
//! stamp the shared Dummy, so each search detaches it
//! (`SharedCellDummy::detached`). The searches share each real cell's and
//! blocker's Can_Enter_Cell facts, read once a batch
//! ([`crate::sim::world::EntryFacts`]); the Dummy is read at each use.
//!
//! Residuals: batched searches don't stamp the shared Dummy's coordinate, so
//! a later read that doesn't stamp first sees the last serial stamp. Mark0's
//! recalculation of the Foot's cell normally re-derives the same attributes;
//! the code-2 chain reads that cell's bridge bit and level before it. A
//! terrain object's multi-cell foundation test sees the Foot still in its
//! cell when the foundation covers it.

use crate::util::time::{Duration, Instant};
use std::cell::Cell;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};

use super::foot_path::MoverLift;
use super::movement_path::MovePathFailure;
use super::movement_tick::FootPathRequest;
use crate::map::entities::EntityCategory;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::locomotor_type::{LocomotorKind, MovementZone, SpeedType};
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::TargetKind;
use crate::sim::components::{DriveCoord, NavTargetRef};
use crate::sim::entity_store::EntityStore;
use crate::sim::game_entity::GameEntity;
use crate::sim::intern::InternedId;
use crate::sim::mission::MissionId;
use crate::sim::overlay_grid::OverlayGrid;
use crate::sim::pathfinding::zone_search::PathSearchFailure;
use crate::sim::pathfinding::{BlockerNeighborCounts, PathGrid, search_steps};
use crate::sim::radio::Contacts;
use crate::sim::world::{EntryFacts, Simulation};

/// The most registrations one turn of a live object pass takes: the turn of
/// no-queue requests and recursions, and each code-2 re-path turn; see the
/// module docs. Ordinary battles stay under it, and a turn of this many
/// searches leaves most of a frame's budget for the rest of the frame at the
/// contract's scale.
const SEARCHES_PER_PASS: usize = 256;

/// The expansions a pass's code-2 re-paths at one urgency make before it
/// starts no more of them; see the module docs. One cell search at A*'s
/// limit fits.
const REPATH_STEPS_PER_PASS: u64 = crate::sim::pathfinding::MAX_SEARCH_NODES as u64;

/// Passes whose durations the batch keeps for its mean and peak.
const PASS_WINDOW: usize = 64;

/// What the latest path batch pass did: the dashboard's pathfinding line.
///
/// Every count is one the pass already makes for its own caps, and the
/// durations are its own wall clock. Nothing in the simulation reads any of
/// this, and it is neither persisted nor state-hashed, so it moves no order
/// and no hash.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PathBatchStats {
    /// Registrations waiting when the latest pass started.
    pub waiting: u32,
    /// Registrations the latest pass searched: after the per-pass cap, before
    /// the re-path budgets' cuts drop answers.
    pub searched: u32,
    /// Registrations it answered, whether or not the Foot took the answer.
    pub answered: u32,
    /// Registrations it left with no answer, so they register again next
    /// frame: the cap, a re-path turn or a re-path budget cut them out.
    pub left_out: u32,
    /// Cells every search of the latest pass expanded.
    pub steps: u64,
    /// Of those, the urgency-2 re-paths' expansions.
    pub urgency2_steps: u64,
    /// Of those, the urgency-1 re-paths' expansions.
    #[serde(default)]
    pub urgency1_steps: u64,
    /// Milliseconds the latest pass took, and the mean and slowest over the
    /// kept window.
    pub last_ms: f64,
    pub mean_ms: f64,
    pub peak_ms: f64,
    /// Passes the window holds.
    pub samples: u32,
}

/// The batch's own observation of its passes; see [`PathBatchStats`].
#[derive(Clone, Debug)]
struct PassWindow {
    waiting: u32,
    searched: u32,
    answered: u32,
    steps: u64,
    urgency2_steps: u64,
    urgency1_steps: u64,
    last_ns: u64,
    ns: [u64; PASS_WINDOW],
    next: usize,
    filled: usize,
}

impl Default for PassWindow {
    fn default() -> Self {
        Self {
            waiting: 0,
            searched: 0,
            answered: 0,
            steps: 0,
            urgency2_steps: 0,
            urgency1_steps: 0,
            last_ns: 0,
            ns: [0; PASS_WINDOW],
            next: 0,
            filled: 0,
        }
    }
}

impl PassWindow {
    fn finish(
        &mut self,
        took: Duration,
        waiting: usize,
        searched: usize,
        answered: usize,
        steps: u64,
        [urgency2_steps, urgency1_steps]: [u64; 2],
    ) {
        let cast = |count: usize| u32::try_from(count).unwrap_or(u32::MAX);
        self.waiting = cast(waiting);
        self.searched = cast(searched);
        self.answered = cast(answered);
        self.steps = steps;
        self.urgency2_steps = urgency2_steps;
        self.urgency1_steps = urgency1_steps;
        self.last_ns = u64::try_from(took.as_nanos()).unwrap_or(u64::MAX);
        self.ns[self.next] = self.last_ns;
        self.next = (self.next + 1) % PASS_WINDOW;
        if self.filled < PASS_WINDOW {
            self.filled += 1;
        }
    }

    fn stats(&self) -> PathBatchStats {
        let ms = |ns: u64| ns as f64 / 1.0e6;
        let window = &self.ns[..self.filled];
        let total_ns: u64 = window.iter().sum();
        PathBatchStats {
            waiting: self.waiting,
            searched: self.searched,
            answered: self.answered,
            left_out: self.waiting.saturating_sub(self.answered),
            steps: self.steps,
            urgency2_steps: self.urgency2_steps,
            urgency1_steps: self.urgency1_steps,
            last_ms: ms(self.last_ns),
            mean_ms: if window.is_empty() {
                0.0
            } else {
                ms(total_ns) / window.len() as f64
            },
            peak_ms: ms(window.iter().max().copied().unwrap_or(0)),
            samples: self.filled as u32,
        }
    }
}

/// The path searches waiting for the next frame, and this frame's answers.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub(crate) struct PathBatch {
    /// Foots whose request waited this frame, and which request each repeats.
    pending: Vec<(u64, WaitingRequest)>,
    /// The urgency of each code-2 re-path the last pass left out, which the
    /// Foot's registrations this frame keep.
    left_out_code2: BTreeMap<u64, u8>,
    /// The recursions the last pass left out, by Foot, which the Foot's
    /// no-queue registrations this frame repeat.
    #[serde(default)]
    left_out_recursions: BTreeMap<u64, WaitingRequest>,
    /// The lowest Foot id the next capped pass answers first; see
    /// [`take_turn`].
    next_turn: u64,
    /// The lowest Foot id each re-path turn, urgency 2's then urgency 1's,
    /// takes first at the next pass: the first re-path a pass left out.
    next_repath: [u64; 2],
    /// This live object pass's answers by Foot. Empty outside the pass, so a
    /// request made between passes waits whether or not the game was saved
    /// in between.
    #[serde(skip)]
    ready: BTreeMap<u64, ReadySlot>,
    /// The kept cell and blocker entry facts of the last pass, or of the
    /// last search a Foot ran at its own turn, emptied before each
    /// ([`Simulation::empty_entry_facts`]).
    #[serde(skip)]
    entry_facts: EntryFacts,
    /// Answers Find_Path used, by [`WaitingRequest`].
    #[cfg(any(test, feature = "test-support"))]
    #[serde(skip)]
    used: [usize; 2],
    /// The Foots the latest pass answered.
    #[cfg(any(test, feature = "test-support"))]
    #[serde(skip)]
    answered: Vec<u64>,
    /// The latest pass's own counts and durations, for the dashboard. Neither
    /// persisted nor state-hashed: no simulation code reads it.
    #[serde(skip)]
    stats: PassWindow,
}

impl std::hash::Hash for PathBatch {
    /// The registrations, the left-out re-paths' urgencies and recursions,
    /// and where the turns start next, which carry into the next frame;
    /// answers live within one.
    /// A cap or budget that never bound leaves no re-path out and the turns
    /// at zero, which fold nothing.
    fn hash<H: std::hash::Hasher>(&self, hasher: &mut H) {
        (self.pending.len() as u64).hash(hasher);
        for &(id, request) in &self.pending {
            id.hash(hasher);
            request.hash_bytes().hash(hasher);
        }
        if !self.left_out_code2.is_empty() {
            (self.left_out_code2.len() as u64).hash(hasher);
            for (&id, &urgency) in &self.left_out_code2 {
                id.hash(hasher);
                urgency.hash(hasher);
            }
        }
        if !self.left_out_recursions.is_empty() {
            (self.left_out_recursions.len() as u64).hash(hasher);
            for (&id, &request) in &self.left_out_recursions {
                id.hash(hasher);
                request.hash_bytes().hash(hasher);
            }
        }
        if self.next_turn != 0 {
            self.next_turn.hash(hasher);
        }
        if self.next_repath != [0; 2] {
            self.next_repath.hash(hasher);
        }
    }
}

/// One turn of a pass over `pending`, one registration per Foot in
/// ascending id: from `next` on, wrapping round to the lowest ids, the first
/// `limit` it takes, in that order, and the rest it leaves out; `next` then
/// moves to the first one left out, if any.
fn take_turn(
    mut pending: Vec<(u64, WaitingRequest)>,
    next: &mut u64,
    limit: usize,
) -> (Vec<(u64, WaitingRequest)>, Vec<(u64, WaitingRequest)>) {
    let split = pending.partition_point(|&(id, _)| id < *next);
    pending.rotate_left(split);
    let left_out = pending.split_off(limit.min(pending.len()));
    if let Some(&(id, _)) = left_out.first() {
        *next = id;
    }
    (pending, left_out)
}

/// The Find_Path request a waiting Foot repeats at its next frame's turn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum WaitingRequest {
    /// The no-queue request, `Find_Path(cell, 0, 0)` toward Walk's
    /// destination or Drive/Ship's track destination.
    NoQueue,
    /// A code-2 ladder's re-path, at the urgency the ladder asked; see the
    /// module docs.
    Code2Ladder { urgency: u8 },
    /// The no-queue request of a refused-step retry recursion: Walk's
    /// (`WalkLocomotionClass::Process(0)` at 0x75B716, 0x75BABB and
    /// 0x75BC04) or Drive/Ship's (`Process_Movement(0, 0)` at 0x4B397F,
    /// 0x4B4480 and 0x4B4552; Ship 0x6A2FCE, 0x6A3AAC and 0x6A3B7E): the
    /// request that takes its answer resumes with the retry spent; see the
    /// module docs.
    RetryRecursion,
    /// The no-queue request of Drive/Ship's second-stage code-4/5 re-entry,
    /// `Process_Movement(allow_retry, 1)` once it cleared the path (Drive
    /// 0x4B41B3..0x4B41F7, Ship 0x6A37DF..0x6A3823): the request that takes
    /// its answer resumes with those arguments; see the module docs.
    StraightReentry { allow_retry: bool },
}

impl WaitingRequest {
    /// The re-path turn this registration takes, in the order a pass starts
    /// them: 0 for a code-2 re-path at urgency 2, which routes round every
    /// moving friendly, 1 for one at urgency 1; none for the rest.
    fn repath_turn(self) -> Option<usize> {
        match self {
            Self::Code2Ladder { urgency: 2 } => Some(0),
            Self::Code2Ladder { .. } => Some(1),
            Self::NoQueue | Self::RetryRecursion | Self::StraightReentry { .. } => None,
        }
    }

    /// Whether this is a recursion's no-queue request, which the request
    /// that takes its answer resumes as.
    fn is_recursion(self) -> bool {
        matches!(self, Self::RetryRecursion | Self::StraightReentry { .. })
    }

    /// Which request this is: 0 no-queue (a recursion's included), 1 code-2
    /// re-path.
    #[cfg(any(test, feature = "test-support"))]
    fn kind(self) -> usize {
        match self {
            Self::NoQueue | Self::RetryRecursion | Self::StraightReentry { .. } => 0,
            Self::Code2Ladder { .. } => 1,
        }
    }

    /// The bytes the batch's hash folds.
    fn hash_bytes(self) -> [u8; 2] {
        match self {
            Self::NoQueue => [0, 0],
            Self::Code2Ladder { urgency } => [1, urgency],
            Self::RetryRecursion => [2, 0],
            Self::StraightReentry { allow_retry } => [3, u8::from(allow_retry)],
        }
    }
}

/// The urgency of a code-2 re-path; `None` for a no-queue request.
fn code2_urgency(&(id, request): &(u64, WaitingRequest)) -> Option<(u64, u8)> {
    match request {
        WaitingRequest::Code2Ladder { urgency } => Some((id, urgency)),
        WaitingRequest::NoQueue
        | WaitingRequest::RetryRecursion
        | WaitingRequest::StraightReentry { .. } => None,
    }
}

/// Whether a re-path falls past its turn's cut whatever it makes: the
/// counts of the re-paths `before` it in its turn reach `budget`. Each count
/// is one its re-path has reached, never more than its search makes in all,
/// so their sum is at most the one the cut adds up.
fn past_cut(before: &[AtomicU64], budget: u64) -> bool {
    before
        .iter()
        .map(|made| made.load(Ordering::Relaxed))
        .sum::<u64>()
        >= budget
}

/// What a pass did with a registration it took.
enum Turn {
    /// It searched: the answer, `None` when the batch had nothing to answer.
    Searched(Option<BatchedSearch>),
    /// Its re-path turn's budget ran out before it.
    OverBudget,
}

/// A Foot's entry in this pass's answers.
#[derive(Clone, Debug, PartialEq)]
struct ReadySlot {
    /// Whether one of the Foot's requests this frame went on to Find_Path;
    /// any later one waits for the next batch.
    claimed: bool,
    /// The registration the pass answered.
    request: WaitingRequest,
    /// The search, until Find_Path takes it; `None` when the batch had
    /// nothing to answer.
    answer: Option<BatchedSearch>,
}

#[derive(Clone, Debug, PartialEq)]
struct BatchedSearch {
    key: FootSearchKey,
    found: Result<Vec<(u16, u16)>, MovePathFailure>,
}

/// What a core search reads from its request and its mover besides the
/// rest of the world: an answer serves only a request with the same key.
#[derive(Clone, Debug, PartialEq, Eq)]
struct FootSearchKey {
    location: (i32, i32, i32),
    start: (u16, u16),
    on_bridge: bool,
    facing: u16,
    goal: (i32, i32, i32),
    urgency: u8,
    category: EntityCategory,
    type_ref: InternedId,
    speed_type: Option<SpeedType>,
    movement_zone: MovementZone,
    crushers: (bool, bool),
    locomotor: Option<LocomotorKind>,
    allow_zone_hierarchy: bool,
    threat_coefficient: u64,
    owner: InternedId,
    veterancy: u16,
    in_playfield: bool,
    mission: MissionId,
    nav_com: Option<NavTargetRef>,
    attack_target: Option<TargetKind>,
    radio_contacts: Contacts,
}

/// The destination a code-2 ladder searches toward: Walk's destination or
/// Drive/Ship's track destination, the null coordinate (0,0,0) without one.
/// No ladder tests it (Walk `0x0075B8A9..`, Drive `0x004B39D3`, Ship
/// `0x006A3022`), so a destination cleared earlier in the same call searches
/// cell (0,0), which Find_Path's precheck refuses (#1672).
pub(super) fn code2_ladder_destination(actor: &GameEntity) -> Option<DriveCoord> {
    let loco = actor.locomotor.as_ref()?;
    match loco.active_kind() {
        LocomotorKind::Walk => Some(
            loco.walk_destination()
                .unwrap_or(super::foot_coordinate::NULL_COORD),
        ),
        LocomotorKind::Drive | LocomotorKind::Ship => {
            Some(super::track_path::track_destination_or_null(actor))
        }
        _ => None,
    }
}

/// The destination a no-queue request searches toward: Walk's destination
/// or Drive/Ship's track destination.
fn no_queue_destination(actor: &GameEntity) -> Option<DriveCoord> {
    let loco = actor.locomotor.as_ref()?;
    match loco.active_kind() {
        LocomotorKind::Walk => loco.walk_destination(),
        LocomotorKind::Drive | LocomotorKind::Ship => super::track_path::track_destination(actor),
        _ => None,
    }
}

/// Whether `foot`'s `request` heads for the cell the Foot stands in: its
/// core search would start in its goal cell. Such a request never waits;
/// see the module docs.
fn heads_for_own_cell(
    foot: &GameEntity,
    request: WaitingRequest,
    terrain: Option<&ResolvedTerrainGrid>,
) -> bool {
    let destination = match request {
        WaitingRequest::Code2Ladder { .. } => code2_ladder_destination(foot),
        WaitingRequest::NoQueue
        | WaitingRequest::RetryRecursion
        | WaitingRequest::StraightReentry { .. } => no_queue_destination(foot),
    };
    destination.is_some_and(|destination| {
        let (start, goal) = super::movement_tick::core_search_cells(foot, destination, terrain);
        start == goal
    })
}

impl Simulation {
    /// Batch the path searches from now on; see the module docs.
    pub fn batch_path_searches(&mut self) {
        self.path_batch.get_or_insert_with(PathBatch::default);
    }

    /// Whether `id`'s `request` waits for the next frame's batch, as if its
    /// movement timer still ran. It does, and registers, when batching is
    /// on, the batch can search this map, `id` has no unclaimed answer this
    /// frame, and the request heads for a cell other than the Foot's own
    /// (see the module docs); otherwise it does not wait, and claims the
    /// unclaimed answer if there is one. A code-2 re-path the last pass left
    /// out registers at the urgency it had, and a no-queue request of a Foot
    /// whose recursion it left out registers as that recursion.
    pub(super) fn defer_path_search(
        path_batch: &mut Option<PathBatch>,
        resolved_terrain: &Option<ResolvedTerrainGrid>,
        overlay_grid: &Option<OverlayGrid>,
        entities: &EntityStore,
        id: u64,
        request: WaitingRequest,
    ) -> bool {
        if overlay_grid.is_none() || resolved_terrain.is_none() {
            return false;
        }
        let Some(batch) = path_batch.as_mut() else {
            return false;
        };
        if let Some(slot) = batch.ready.get_mut(&id)
            && !slot.claimed
        {
            slot.claimed = true;
            return false;
        }
        if entities
            .get(id)
            .is_some_and(|foot| heads_for_own_cell(foot, request, resolved_terrain.as_ref()))
        {
            return false;
        }
        let request = match (request, batch.left_out_code2.get(&id)) {
            (WaitingRequest::Code2Ladder { .. }, Some(&urgency)) => {
                WaitingRequest::Code2Ladder { urgency }
            }
            (WaitingRequest::NoQueue, _) => batch
                .left_out_recursions
                .get(&id)
                .copied()
                .unwrap_or(request),
            _ => request,
        };
        batch.pending.push((id, request));
        true
    }

    /// The recursion whose answer `id`'s next request this frame takes, if
    /// the pass answered one: that request resumes as the recursion runs.
    pub(super) fn answered_recursion(&self, id: u64) -> Option<WaitingRequest> {
        self.path_batch
            .as_ref()?
            .ready
            .get(&id)
            .filter(|slot| !slot.claimed && slot.request.is_recursion())
            .map(|slot| slot.request)
    }

    /// The urgency `id`'s code-2 ladder searches at, given `now`, the
    /// urgency it asks this frame: the urgency of the re-path this pass
    /// answered, if it answered one; see the module docs.
    pub(super) fn batched_code2_urgency(&self, id: u64, now: u8) -> u8 {
        match self
            .path_batch
            .as_ref()
            .and_then(|batch| batch.ready.get(&id))
            .map(|slot| slot.request)
        {
            Some(WaitingRequest::Code2Ladder { urgency }) => urgency,
            _ => now,
        }
    }

    /// The batched answer for `request`'s core search toward `goal`, if one
    /// was computed for this request.
    pub(super) fn take_batched_search(
        &mut self,
        request: &FootPathRequest,
        goal: DriveCoord,
    ) -> Option<Result<Vec<(u16, u16)>, MovePathFailure>> {
        let id = request.entity_id;
        if !self
            .path_batch
            .as_ref()?
            .ready
            .get(&id)
            .is_some_and(|slot| slot.answer.is_some())
        {
            return None;
        }
        let key = self.foot_search_key(request, goal)?;
        let batch = self.path_batch.as_mut()?;
        let slot = batch.ready.get_mut(&id)?;
        if slot.answer.as_ref()?.key != key {
            return None;
        }
        let answer = slot.answer.take()?;
        #[cfg(any(test, feature = "test-support"))]
        {
            batch.used[slot.request.kind()] += 1;
        }
        Some(answer.found)
    }

    /// The start of the live object pass: search for the Foots registered
    /// last frame, at most [`SEARCHES_PER_PASS`] of them besides their code-2
    /// re-paths, which take a turn per urgency within
    /// [`REPATH_STEPS_PER_PASS`] each.
    pub(crate) fn answer_batched_path_searches(
        &mut self,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) {
        let _area = crate::sim::frame_profile::area(crate::sim::frame_profile::phase::PATHFINDING);
        self.answer_path_searches_within(rules, registry, REPATH_STEPS_PER_PASS);
    }

    /// [`Self::answer_batched_path_searches`] with a budget of
    /// `repath_steps` expansions, at least 1, for each re-path turn.
    fn answer_path_searches_within(
        &mut self,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        repath_steps: u64,
    ) {
        let Some(batch) = self.path_batch.as_mut() else {
            return;
        };
        let started = Instant::now();
        batch.ready.clear();
        #[cfg(any(test, feature = "test-support"))]
        batch.answered.clear();
        let pending = std::mem::take(&mut batch.pending);
        // Last pass's left-outs went into this frame's registrations.
        batch.left_out_code2.clear();
        batch.left_out_recursions.clear();
        if pending.is_empty() {
            batch.stats.finish(started.elapsed(), 0, 0, 0, 0, [0; 2]);
            return;
        }
        // A Foot's latest registration this frame stands: its latest state
        // makes the request it repeats.
        let pending: Vec<(u64, WaitingRequest)> = pending
            .into_iter()
            .collect::<BTreeMap<_, _>>()
            .into_iter()
            .collect();
        let waiting = pending.len();
        let mut repath_turns: [Vec<(u64, WaitingRequest)>; 2] = Default::default();
        let mut others = Vec::new();
        for entry in pending {
            match entry.1.repath_turn() {
                Some(turn) => repath_turns[turn].push(entry),
                None => others.push(entry),
            }
        }
        // The code-2 re-paths this pass leaves out keep their urgencies.
        let mut turn = Vec::new();
        let mut repaths = [0; 2];
        for ((count, next), taken) in repaths
            .iter_mut()
            .zip(&mut batch.next_repath)
            .zip(repath_turns)
        {
            let (taken, left_out) = take_turn(taken, next, SEARCHES_PER_PASS);
            batch
                .left_out_code2
                .extend(left_out.iter().filter_map(code2_urgency));
            *count = taken.len();
            turn.extend(taken);
        }
        // The recursions it leaves out stay recursions.
        let (others, left_out) = take_turn(others, &mut batch.next_turn, SEARCHES_PER_PASS);
        batch.left_out_recursions = left_out
            .into_iter()
            .filter(|&(_, request)| request.is_recursion())
            .collect();
        turn.extend(others);
        let searched = turn.len();
        let (turns, expanded, repaths_made) = match self.path_grid_snapshot() {
            Some(grid) if self.overlay_grid.is_some() && self.resolved_terrain.is_some() => {
                self.search_batch(&turn, repaths, repath_steps, rules, registry, &grid)
            }
            _ => (Vec::new(), 0, [0; 2]),
        };
        let mut turns = turns.into_iter();
        let batch = self.path_batch.as_mut().expect("batching is on");
        let mut cut = [false; 2];
        for (id, request) in turn {
            match turns.next().unwrap_or(Turn::Searched(None)) {
                Turn::Searched(answer) => {
                    let slot = ReadySlot {
                        claimed: false,
                        request,
                        answer,
                    };
                    batch.ready.insert(id, slot);
                }
                // No slot: the Foot's requests this frame register again,
                // still at the re-path's urgency, and the first the budget
                // cuts starts its turn at the next pass.
                Turn::OverBudget => {
                    batch.left_out_code2.extend(code2_urgency(&(id, request)));
                    if let Some(index) = request.repath_turn()
                        && !std::mem::replace(&mut cut[index], true)
                    {
                        batch.next_repath[index] = id;
                    }
                }
            }
        }
        #[cfg(any(test, feature = "test-support"))]
        {
            batch.answered = batch.ready.keys().copied().collect();
        }
        let answered = batch.ready.len();
        batch.stats.finish(
            started.elapsed(),
            waiting,
            searched,
            answered,
            expanded,
            repaths_made,
        );
    }

    /// What the latest path batch pass did: its own counts and durations.
    /// `None` while the simulation searches serially, since then no pass runs.
    pub fn path_batch_stats(&self) -> Option<PathBatchStats> {
        Some(self.path_batch.as_ref()?.stats.stats())
    }

    /// Actor ids whose requests are waiting for the next live object pass.
    /// A Foot can register more than once in a pass; callers needing a set
    /// deduplicate this read-only projection of the existing pending queue.
    pub fn pending_path_search_actor_ids(&self) -> impl Iterator<Item = u64> + '_ {
        self.path_batch
            .iter()
            .flat_map(|batch| batch.pending.iter().map(|&(id, _)| id))
    }

    /// Empty the batch's entry-fact tables ([`EntryFacts`]) before a batch of
    /// core searches, or one a Foot runs at its own turn, reads the world:
    /// until the world changes, the searches keep the facts they read there
    /// ([`Self::entry_facts`]). Nothing while batching is off.
    pub(super) fn empty_entry_facts(&mut self) {
        let (Some(terrain), Some(batch)) =
            (self.resolved_terrain.as_ref(), self.path_batch.as_mut())
        else {
            return;
        };
        batch
            .entry_facts
            .reset(terrain.cells().len(), self.substrate.entities.slot_count());
    }

    /// The tables a core search keeps its entry facts in, emptied by
    /// [`Self::empty_entry_facts`] since the world last changed; `None`
    /// while batching is off.
    pub(super) fn entry_facts(&self) -> Option<&EntryFacts> {
        Some(&self.path_batch.as_ref()?.entry_facts)
    }

    /// The end of the live object pass: its answers expire.
    pub(crate) fn finish_batched_path_searches(&mut self) {
        if let Some(batch) = self.path_batch.as_mut() {
            batch.ready.clear();
        }
    }

    /// The searches for `turn`, which starts with its re-path turns in
    /// order, `repaths` registrations each, with `repath_steps` for each
    /// turn's budget. Returns its turns, the cells every search expanded, and
    /// each re-path turn's share of them.
    fn search_batch(
        &mut self,
        turn: &[(u64, WaitingRequest)],
        repaths: [usize; 2],
        repath_steps: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        grid: &PathGrid,
    ) -> (Vec<Turn>, u64, [u64; 2]) {
        // One blocker plane for every search. With an overlay grid movers
        // reach it through their Foot+55C counters, which Mark0 leaves alone,
        // so it is the plane each Find_Path would read after its Mark0.
        self.movement_pass_cache.blocker_plane(
            &mut self.substrate.entities,
            grid,
            self.resolved_terrain.as_ref(),
            &self.terrain_objects,
            self.overlay_grid.as_ref(),
            &self.interner,
            rules,
        );
        // Every search reads the same world, so a cell's or blocker's entry
        // facts are read once a batch and shared.
        self.empty_entry_facts();
        let sim = &*self;
        let counts = sim.movement_pass_cache.current_blocker_plane();
        let facts = sim.entry_facts().expect("batching is on");
        let dummy = sim
            .resolved_terrain
            .as_ref()
            .expect("checked by the caller")
            .shared_cell_dummy();
        // Main is shared with retained playback, which may draw on another
        // thread, so only tests check that the searches draw nothing.
        #[cfg(any(test, feature = "test-support"))]
        let rng = sim.rng_state();
        // Urgency-2 re-paths route around every moving friendly and cost the
        // most by far (match bench: a median of ~720 expansions against ~70
        // for the rest), so they start first, then the urgency-1 ones, each
        // turn in its order, and none runs alone at the end. An answer
        // depends on its request alone, so the order changes none. `made`
        // holds each re-path's expansions so far, which it publishes as it
        // searches: one whose predecessors in its turn reach the budget falls
        // past the cut below whatever it makes, so it does not start, or
        // stops. No search runs other pool work on its thread, which would
        // count there.
        let budgeted = repaths[0] + repaths[1];
        let made: Vec<AtomicU64> = (0..budgeted).map(|_| AtomicU64::new(0)).collect();
        let mut searched = crate::sim::parallel::map_in_order(turn, |index, &(id, request)| {
            // The re-paths before this one in its turn.
            let start = if index < repaths[0] { 0 } else { repaths[0] };
            debug_assert_eq!(
                request.repath_turn(),
                (index < budgeted).then_some(usize::from(index >= repaths[0])),
            );
            let base = search_steps();
            let search = |keep_searching: Option<&dyn Fn(u32) -> bool>| {
                dummy.detached(|| {
                    sim.batched_search(
                        id,
                        request,
                        rules,
                        registry,
                        grid,
                        counts,
                        Some(facts),
                        keep_searching,
                    )
                })
            };
            if index >= budgeted {
                let answer = search(None);
                return Some((answer, search_steps() - base, false));
            }
            if past_cut(&made[start..index], repath_steps) {
                return None;
            }
            let stopped = Cell::new(false);
            let answer = search(Some(&|expanded| {
                made[index].store(
                    search_steps() - base + u64::from(expanded),
                    Ordering::Relaxed,
                );
                stopped.set(past_cut(&made[start..index], repath_steps));
                !stopped.get()
            }));
            let steps = search_steps() - base;
            made[index].store(steps, Ordering::Relaxed);
            Some((answer, steps, stopped.get()))
        });
        #[cfg(any(test, feature = "test-support"))]
        assert_eq!(rng, sim.rng_state(), "batched searches draw no RNG");
        // Expansions the pass actually made, counted before the cut drops the
        // answers past the budget: that work was still done.
        let expanded: u64 = searched.iter().flatten().map(|entry| entry.1).sum();
        // Each turn's cut, which depends on its re-paths' expansions alone: in
        // turn order a re-path is answered while the ones before it stay
        // under the budget.
        let mut repaths_made = [0; 2];
        let mut start = 0;
        for (turn_made, count) in repaths_made.iter_mut().zip(repaths) {
            let end = start + count;
            *turn_made = made[start..end]
                .iter()
                .map(|made| made.load(Ordering::Relaxed))
                .sum();
            let mut steps = 0;
            for searched in &mut searched[start..end] {
                if steps < repath_steps {
                    let &(_, made, stopped) = searched
                        .as_ref()
                        .expect("a re-path before the cut was searched");
                    assert!(!stopped, "a re-path before the cut searched to its end");
                    steps += made;
                } else {
                    *searched = None;
                }
            }
            start = end;
        }
        let turns = searched
            .into_iter()
            .map(|searched| {
                searched.map_or(Turn::OverBudget, |(answer, ..)| Turn::Searched(answer))
            })
            .collect();
        (turns, expanded, repaths_made)
    }

    /// `id`'s search as its `request`'s Find_Path would run it now, without
    /// effects, reading the batch's kept entry `facts`; `None` when it has
    /// nothing to answer or `keep_searching` stopped it.
    fn batched_search(
        &self,
        id: u64,
        request: WaitingRequest,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        grid: &PathGrid,
        counts: &BlockerNeighborCounts,
        facts: Option<&EntryFacts>,
        keep_searching: Option<&dyn Fn(u32) -> bool>,
    ) -> Option<BatchedSearch> {
        let request = match request {
            WaitingRequest::Code2Ladder { urgency } => self.code2_ladder_request(id, urgency)?,
            WaitingRequest::NoQueue
            | WaitingRequest::RetryRecursion
            | WaitingRequest::StraightReentry { .. } => self.no_queue_request(id)?,
        };
        let goal = request.destination;
        let answer = self
            .find_path_target_answer(id, goal, rules, registry)
            .ok()??;
        if self
            .find_path_redirect(id, goal, answer, rules)
            .ok()?
            .is_some()
        {
            return None;
        }
        let (found, read_own_cell) = self.foot_core_search(
            &request,
            goal,
            rules,
            registry,
            grid,
            counts,
            MoverLift::Emulated,
            facts,
            keep_searching,
        );
        if read_own_cell
            || matches!(
                found,
                Err(MovePathFailure::MissingGrid
                    | MovePathFailure::Search(
                        PathSearchFailure::CellEntryUnavailable(_) | PathSearchFailure::Stopped
                    ))
            )
        {
            return None;
        }
        Some(BatchedSearch {
            key: self.foot_search_key(&request, goal)?,
            found,
        })
    }

    /// The no-queue request `id` makes now: Walk's toward its destination,
    /// Drive/Ship's toward its track destination, at urgency 0. Its calls are
    /// `FootClass::Find_Path` (`0x004D3920`) at Walk `0x0075AFC5`, Drive
    /// `0x004B28A3` and Ship `0x006A1EF3`.
    fn no_queue_request(&self, id: u64) -> Option<FootPathRequest> {
        let destination = no_queue_destination(self.substrate.entities.get(id)?)?;
        FootPathRequest::track(
            &self.substrate.entities,
            id,
            destination,
            0,
            self.playfield_bounds(),
        )
    }

    /// The request `id`'s code-2 ladder makes now, at `urgency`. Drive's
    /// ladder is `DriveLocomotionClass::Process_Movement` (`0x004B2630`)'s
    /// `0x004B3607..0x004B3A94`; Walk's is `0x0075B8A9..0x0075B9F9`.
    fn code2_ladder_request(&self, id: u64, urgency: u8) -> Option<FootPathRequest> {
        let actor = self.substrate.entities.get(id)?;
        let destination = code2_ladder_destination(actor)?;
        FootPathRequest::track(
            &self.substrate.entities,
            id,
            destination,
            urgency,
            self.playfield_bounds(),
        )
    }

    /// The key of `request`'s core search toward `goal` as the world stands.
    /// VERA-only: matches a batched answer to its request; the live mover and
    /// world are the source of truth, so an answer serves only an equal key.
    fn foot_search_key(
        &self,
        request: &FootPathRequest,
        goal: DriveCoord,
    ) -> Option<FootSearchKey> {
        let actor = self.substrate.entities.get(request.entity_id)?;
        let snap = request.snapshot();
        let location = super::ground_pose::position_world_coord(&actor.position);
        Some(FootSearchKey {
            location: (location.x, location.y, location.z),
            start: super::movement_tick::core_search_cells(
                actor,
                goal,
                self.resolved_terrain.as_ref(),
            )
            .0,
            on_bridge: actor.on_bridge(),
            facing: actor.body_facing.current(self.session.binary_frame()),
            goal: (goal.x, goal.y, goal.z),
            urgency: request.urgency,
            category: snap.category,
            type_ref: actor.type_ref(),
            speed_type: snap.speed_type,
            movement_zone: snap.movement_zone,
            crushers: (snap.regular_crusher, snap.omni_crusher),
            locomotor: snap.locomotor.as_ref().map(|loco| loco.active_kind()),
            allow_zone_hierarchy: self.foot_search_allows_hierarchy(request),
            threat_coefficient: self.foot_path_threat_coefficient(request.entity_id).bits(),
            owner: actor.owner(),
            veterancy: actor.veterancy.rank(),
            in_playfield: actor.in_playfield(),
            mission: actor.mission.current(),
            nav_com: actor.navigation.nav_com(),
            attack_target: actor.attack_target.target(),
            radio_contacts: actor.radio_contacts.clone(),
        })
    }
}

#[cfg(test)]
#[path = "path_batch_tests.rs"]
mod tests;
