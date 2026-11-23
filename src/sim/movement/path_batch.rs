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
//! blocker's Can_Enter_Cell facts and each cell's code-2 chain, kept from
//! their first read in a batch ([`crate::sim::world::EntryFacts`]); the Dummy
//! is read at each use.
//!
//! Residuals: batched searches don't stamp the shared Dummy's coordinate, so
//! a later read that doesn't stamp first sees the last serial stamp. Mark0's
//! recalculation of the Foot's cell normally re-derives the same attributes;
//! the code-2 chain reads that cell's bridge bit and level before it. A
//! terrain object's multi-cell foundation test sees the Foot still in its
//! cell when the foundation covers it.

use crate::util::time::{Duration, Instant};
use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use super::foot_path::MoverLift;
use super::movement_path::MovePathFailure;
use super::movement_tick::FootPathRequest;
use crate::map::entities::EntityCategory;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::locomotor_type::{LocomotorKind, MovementZone, SpeedType};
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::TargetKind;
use crate::sim::components::{DriveCoord, NavTargetRef};
use crate::sim::entity_store::EntityStore;
use crate::sim::game_entity::GameEntity;
use crate::sim::intern::InternedId;
use crate::sim::mission::MissionId;
use crate::sim::overlay_grid::OverlayGrid;
use crate::sim::pathfinding::zone_search::PathSearchFailure;
use crate::sim::pathfinding::{BlockerNeighborCounts, GoalField, PathGrid, search_steps};
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

/// The factor a field's build has to clear before it is worth building.
///
/// The build visits every cell of the ground plane once. The searches it
/// replaces ask the Foot about the eight neighbours of every cell they
/// expand, and one such ask costs about as much as eight visits of the
/// plane: the ask reads the cell, the object list and the mover's speed
/// row, where a visit reads a bit. So a build repays itself when the
/// group's own paths cover the map this many times over.
const FIELD_BUILD_WORTH: u64 = 64;

/// The fields a pass builds at most. A pass that built a field for every
/// goal it served would pay the build again for every goal the cache had
/// dropped, so a pass builds few and the cache keeps them.
const BUILDS_PER_PASS: usize = 2;

/// The shared goal fields a pass used, and the ones it built.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct SharedFieldCounts {
    /// The fields the pass used, one per goal group it served.
    pub used: u32,
    /// The cells those fields cover: the ground plane's cells that reach
    /// the goal, which the walks read instead of searching.
    pub cells: u64,
    /// The fields the pass built. A field the batch already held costs no
    /// build.
    pub built: u32,
    /// The cells the builds visited.
    pub build_cells: u64,
}

/// The shared goal fields the batch keeps between passes.
///
/// A field is a reverse search over the map's ground plane for one goal
/// cell. The batch keeps it while the plane it describes is the current
/// one, so a mass order that names its goal every frame pays for the build
/// once. The plane's revision names its state, so a field built from
/// another state is dropped rather than read.
#[derive(Clone, Debug, Default)]
struct GoalFieldCache {
    /// The state of the ground walkability plane the fields were built
    /// from.
    revision: u64,
    /// The fields by goal cell.
    fields: BTreeMap<(u16, u16), CachedField>,
    /// The goals in use, the most recent first.
    order: Vec<(u16, u16)>,
    /// The bytes the fields hold together.
    bytes: usize,
}

/// One cached field.
#[derive(Clone, Debug)]
struct CachedField {
    field: Arc<GoalField>,
}

impl GoalFieldCache {
    /// The fields the cache holds.
    const CAPACITY: usize = 32;
    /// The bytes the cached fields may hold together. A field holds one byte
    /// per cell of the map it was built over, so this is the cache's memory.
    const MAX_BYTES: usize = 8_000_000;

    /// The field for `goal`, when the cache holds one built from the plane's
    /// current state.
    fn get(&mut self, goal: (u16, u16), revision: u64) -> Option<Arc<GoalField>> {
        if self.revision != revision {
            // The plane moved: every field the cache holds describes the
            // plane as it was.
            self.fields.clear();
            self.order.clear();
            self.bytes = 0;
            self.revision = revision;
            return None;
        }
        let field = self.fields.get(&goal)?.field.clone();
        self.touch(goal);
        Some(field)
    }

    /// Keep `field` for `goal`, dropping the oldest fields when the cache is
    /// full or its memory is spent.
    fn put(&mut self, goal: (u16, u16), revision: u64, field: Arc<GoalField>) {
        self.revision = revision;
        if let Some(old) = self.fields.remove(&goal) {
            self.bytes = self.bytes.saturating_sub(old.field.footprint());
        }
        self.bytes += field.footprint();
        self.fields.insert(
            goal,
            CachedField {
                field: field.clone(),
            },
        );
        self.touch(goal);
        while self.fields.len() > Self::CAPACITY || self.bytes > Self::MAX_BYTES {
            let Some(oldest) = self.order.last().copied() else {
                break;
            };
            self.order.pop();
            if let Some(entry) = self.fields.remove(&oldest) {
                self.bytes = self.bytes.saturating_sub(entry.field.footprint());
            }
        }
    }

    /// Move `goal` to the front of the use order.
    fn touch(&mut self, goal: (u16, u16)) {
        self.order.retain(|held| *held != goal);
        self.order.insert(0, goal);
    }
}

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
    /// The shared goal fields the latest pass used, and the cells they
    /// covered. VERA-only: a field is a reverse search over the map's
    /// ground layer, so its cells are its own work and no search's
    /// expansions.
    #[serde(default)]
    pub shared_fields: u32,
    #[serde(default)]
    pub shared_field_cells: u64,
    /// The shared goal fields the latest pass built, and the cells their
    /// builds visited. A field the batch already held costs no build.
    #[serde(default)]
    pub shared_field_builds: u32,
    #[serde(default)]
    pub shared_field_build_cells: u64,
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
    shared_fields: u32,
    shared_field_cells: u64,
    shared_field_builds: u32,
    shared_field_build_cells: u64,
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
            shared_fields: 0,
            shared_field_cells: 0,
            shared_field_builds: 0,
            shared_field_build_cells: 0,
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
        shared: SharedFieldCounts,
    ) {
        let cast = |count: usize| u32::try_from(count).unwrap_or(u32::MAX);
        self.waiting = cast(waiting);
        self.searched = cast(searched);
        self.answered = cast(answered);
        self.steps = steps;
        self.urgency2_steps = urgency2_steps;
        self.urgency1_steps = urgency1_steps;
        self.shared_fields = shared.used;
        self.shared_field_cells = shared.cells;
        self.shared_field_builds = shared.built;
        self.shared_field_build_cells = shared.build_cells;
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
            shared_fields: self.shared_fields,
            shared_field_cells: self.shared_field_cells,
            shared_field_builds: self.shared_field_builds,
            shared_field_build_cells: self.shared_field_build_cells,
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
    /// last search a Foot ran at its own turn, and the code-2 chains of the
    /// last pass, each emptied before the next
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
    /// The shared goal fields the batch keeps between passes. Derived from
    /// the map's ground plane, so it is dropped when the plane moves; it
    /// carries no state of its own.
    #[serde(skip)]
    fields: GoalFieldCache,
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
    found: Result<Vec<i32>, MovePathFailure>,
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
        LocomotorKind::Walk | LocomotorKind::Mech => Some(
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
        LocomotorKind::Walk | LocomotorKind::Mech => loco.walk_destination(),
        LocomotorKind::Drive | LocomotorKind::Ship => super::track_path::track_destination(actor),
        _ => None,
    }
}

/// The cell a destination names, as the core search's goal cell
/// ([`super::movement_tick::core_search_cells`]).
fn goal_cell(destination: DriveCoord) -> (u16, u16) {
    ((destination.x / 256) as u16, (destination.y / 256) as u16)
}

/// Whether the shared goal field's plane is the mover's own: the field is
/// built over the ground plane, and the ground plane is the vehicle's. The
/// Infantry class has its own cell gate (Infantry `0x0051BF90`, Unit
/// `0x0073F0A0`), which lets an infantryman into a cell a terrain object
/// stands in, while the field's plane closes that cell to everybody. So the
/// field serves the vehicle class, and an infantryman keeps the stock
/// search.
///
/// The class decides this, not the locomotor: the cell gate is chosen by the
/// mover's class, so a vehicle that walks is still gated as a vehicle and
/// walks the field's plane.
fn field_serves(category: EntityCategory) -> bool {
    category == EntityCategory::Unit
}

/// The waiting registrations by the goal cell their Foots head for, and how
/// many there are of each.
///
/// The demand for a goal is what decides whether a field for it repays its
/// build: the batch answers a capped number of the waiting registrations a
/// pass, so the pass's own group says little about how many searches a
/// field would replace. A code-2 re-path is no demand, because it keeps the
/// stock search, and an infantryman is no demand either, because the field
/// serves vehicles only. A Foot that waits twice is one demand: the batch
/// answers one registration a Foot.
fn goal_demand(
    entities: &EntityStore,
    pending: &[(u64, WaitingRequest)],
) -> BTreeMap<(u16, u16), u32> {
    let mut demand: BTreeMap<(u16, u16), u32> = BTreeMap::new();
    let mut counted: BTreeSet<u64> = BTreeSet::new();
    for (id, request) in pending {
        if matches!(request, WaitingRequest::Code2Ladder { .. }) {
            continue;
        }
        if !counted.insert(*id) {
            continue;
        }
        let Some(actor) = entities.get(*id) else {
            continue;
        };
        if !field_serves(actor.category()) {
            continue;
        }
        let Some(destination) = no_queue_destination(actor) else {
            continue;
        };
        *demand.entry(goal_cell(destination)).or_default() += 1;
    }
    demand
}

/// Whether the cell holds a bridge, whose deck the goal field's ground plane
/// does not describe.
fn cell_has_bridge(grid: &PathGrid, cell: (u16, u16)) -> bool {
    grid.cell(cell.0, cell.1)
        .is_some_and(|cell| cell.bridge_walkable)
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
        rules: &RuleSet,
    ) -> Option<Result<Vec<i32>, MovePathFailure>> {
        // Only ordinary zero-prefix requests register with the batch. A
        // train extending its last queued step has a different search origin.
        if request.retain_head {
            return None;
        }
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
        let key = self.foot_search_key(request, goal, rules)?;
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
    pub(crate) fn answer_batched_path_searches(&mut self, rules: &RuleSet) {
        let _area = crate::sim::frame_profile::area(crate::sim::frame_profile::phase::PATHFINDING);
        self.answer_path_searches_within(rules, REPATH_STEPS_PER_PASS);
    }

    /// [`Self::answer_batched_path_searches`] with a budget of
    /// `repath_steps` expansions, at least 1, for each re-path turn.
    fn answer_path_searches_within(&mut self, rules: &RuleSet, repath_steps: u64) {
        let Some(batch) = self.path_batch.as_mut() else {
            return;
        };
        let started = Instant::now();
        batch.ready.clear();
        #[cfg(any(test, feature = "test-support"))]
        batch.answered.clear();
        // Last pass's left-outs went into this frame's registrations.
        batch.left_out_code2.clear();
        batch.left_out_recursions.clear();
        let pending = &mut batch.pending;
        if pending.is_empty() {
            batch.stats.finish(
                started.elapsed(),
                0,
                0,
                0,
                0,
                [0; 2],
                SharedFieldCounts::default(),
            );
            return;
        }
        // The demand the waiting registrations carry: how many searches a
        // field for a goal would replace. The scan is the pass's own work,
        // so it runs only when the rules ask for a field.
        let demand = if rules.shared_goal_field() {
            goal_demand(&self.substrate.entities, &pending)
        } else {
            BTreeMap::new()
        };
        // Saves and hashes see registrations in arrival order. At the pass,
        // sort stably by Foot id and keep each Foot's latest registration.
        // Drain the buffer into the turns so the next frame can reuse it.
        pending.sort_by_key(|&(id, _)| id);
        pending.dedup_by(|later, earlier| {
            if later.0 == earlier.0 {
                earlier.1 = later.1;
                true
            } else {
                false
            }
        });
        let waiting = pending.len();
        let mut repath_turns: [Vec<(u64, WaitingRequest)>; 2] = Default::default();
        let mut others = Vec::new();
        for entry in pending.drain(..) {
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
        let (turns, expanded, repaths_made, shared) = match self.path_grid_snapshot() {
            Some(grid) if self.overlay_grid.is_some() && self.resolved_terrain.is_some() => {
                self.search_batch(&turn, repaths, repath_steps, rules, &grid, &demand)
            }
            _ => (Vec::new(), 0, [0; 2], SharedFieldCounts::default()),
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
            shared,
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
    /// ([`Self::entry_facts`]). `chains` also empties the code-2 chains,
    /// which only a batch's searches share ([`EntryFacts::code2_chain`]).
    /// Nothing while batching is off.
    pub(super) fn empty_entry_facts(&mut self, chains: bool) {
        let (Some(terrain), Some(batch)) =
            (self.resolved_terrain.as_ref(), self.path_batch.as_mut())
        else {
            return;
        };
        let cells = terrain.cells().len();
        batch
            .entry_facts
            .reset(cells, self.substrate.entities.slot_count());
        if chains {
            batch.entry_facts.reset_chains(cells);
        }
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
        grid: &PathGrid,
        demand: &BTreeMap<(u16, u16), u32>,
    ) -> (Vec<Turn>, u64, [u64; 2], SharedFieldCounts) {
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
        // facts and a cell's code-2 chain are read once a batch and shared.
        self.empty_entry_facts(true);
        // The request each registration searches with. It is read from the
        // actor, which the pass's frozen world keeps still.
        let requests: Vec<Option<FootPathRequest>> = turn
            .iter()
            .map(|&(id, request)| self.batched_request(id, request))
            .collect();
        // The shared goal fields of the pass, one per registration. The
        // fields are built before the searches start, so the batch's cache
        // of them is written here and read by the workers.
        let (fields, shared) = self.shared_goal_fields(&requests, grid, rules, demand);
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
                let Some(request) = requests[index].as_ref() else {
                    return None;
                };
                dummy.detached(|| {
                    // The shared goal field of this registration's goal
                    // cell. The field's plane is the ground layer, so the
                    // groups a field serves leave out the movers on a deck
                    // and those heading for one.
                    let field = fields[index].as_deref();
                    sim.batched_search(
                        id,
                        request,
                        rules,
                        grid,
                        counts,
                        Some(facts),
                        keep_searching,
                        field,
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
        (turns, expanded, repaths_made, shared)
    }

    /// `id`'s search as its `request`'s Find_Path would run it now, without
    /// effects, reading the batch's kept entry `facts`; `None` when it has
    /// nothing to answer or `keep_searching` stopped it.
    fn batched_search(
        &self,
        id: u64,
        request: &FootPathRequest,
        rules: &RuleSet,
        grid: &PathGrid,
        counts: &BlockerNeighborCounts,
        facts: Option<&EntryFacts>,
        keep_searching: Option<&dyn Fn(u32) -> bool>,
        field: Option<&GoalField>,
    ) -> Option<BatchedSearch> {
        let goal = request.destination;
        let answer = self.find_path_target_answer(id, goal, rules).ok()??;
        if self
            .find_path_redirect(id, goal, answer, rules)
            .ok()?
            .is_some()
        {
            return None;
        }
        let (found, read_own_cell) = self.foot_core_search(
            request,
            goal,
            rules,
            grid,
            counts,
            MoverLift::Emulated,
            facts,
            keep_searching,
            field,
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
            key: self.foot_search_key(&request, goal, rules)?,
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

    /// The request `id`'s registration searches with, as the world stands.
    /// `None` when the Foot has no destination to search toward.
    fn batched_request(&self, id: u64, request: WaitingRequest) -> Option<FootPathRequest> {
        match request {
            WaitingRequest::Code2Ladder { urgency } => self.code2_ladder_request(id, urgency),
            WaitingRequest::NoQueue
            | WaitingRequest::RetryRecursion
            | WaitingRequest::StraightReentry { .. } => self.no_queue_request(id),
        }
    }

    /// The shared goal fields of a pass, one per registration, with the
    /// fields the pass used and built.
    ///
    /// The registrations that head for the same goal cell share one field: a
    /// mass order gives every selected unit the same destination, so its
    /// registrations share a goal. A goal whose field the batch already holds
    /// costs nothing to use, so it is used however many registrations share
    /// it; a goal with no field yet gets one only when the searches it would
    /// replace repay the build.
    ///
    /// The field's plane is the map's ground walkability, which a vehicle
    /// walks and an infantryman does not: an infantryman walks where a
    /// terrain object stands, so his field would be another plane. He keeps
    /// the stock search. A mover on a deck, or one heading for a deck, keeps
    /// it as well: the field's plane is the ground one, and a deck is not on
    /// it.
    ///
    /// A field is used only when the group's own paths repay its build, and
    /// the same test governs a field the batch already holds: whether a pass
    /// uses a field then depends on the waiting registrations alone, and not
    /// on what the batch happened to hold before, so a save and a reload
    /// route the same way.
    fn shared_goal_fields(
        &mut self,
        requests: &[Option<FootPathRequest>],
        grid: &PathGrid,
        rules: &RuleSet,
        demand: &BTreeMap<(u16, u16), u32>,
    ) -> (Vec<Option<Arc<GoalField>>>, SharedFieldCounts) {
        let mut fields: Vec<Option<Arc<GoalField>>> = vec![None; requests.len()];
        let mut counts = SharedFieldCounts::default();
        if !rules.shared_goal_field() {
            return (fields, counts);
        }
        // The cell each registration's search starts from, for the estimate
        // of the paths the demand would take and for the deck test.
        let starts: Vec<Option<(u16, u16)>> = requests
            .iter()
            .map(|request| {
                request
                    .as_ref()
                    .and_then(|request| self.request_start_cell(request))
            })
            .collect();
        let mut groups: BTreeMap<(u16, u16), Vec<usize>> = BTreeMap::new();
        for (index, request) in requests.iter().enumerate() {
            let Some(request) = request else {
                continue;
            };
            if !field_serves(request.snapshot().category) {
                continue;
            }
            // A code-2 re-path keeps the stock search: its urgency makes the
            // search route around a jammed cell (the code-2 multiplier,
            // `AStar_compute_edge_cost @ 0x00429830`), which the field's
            // step count does not know.
            if request.urgency != 0 {
                continue;
            }
            let goal = goal_cell(request.destination);
            // The field's plane is the ground one, so a mover on a deck, or
            // one heading for a deck, keeps the stock search. The test is
            // made here, where the groups are formed, so a group that holds
            // only decks takes no build slot.
            if cell_has_bridge(grid, goal)
                || starts[index].is_some_and(|start| cell_has_bridge(grid, start))
            {
                continue;
            }
            groups.entry(goal).or_default().push(index);
        }
        // The goals by the demand they carry, the largest first: a pass
        // builds few fields, and the ones that pay best come first.
        let mut goals: Vec<(u16, u16)> = groups.keys().copied().collect();
        goals.sort_by_key(|goal| std::cmp::Reverse(demand.get(goal).copied().unwrap_or(0)));
        let Some(batch) = self.path_batch.as_mut() else {
            return (fields, counts);
        };
        let revision = grid.revision();
        let mut builds = 0;
        for goal in goals {
            let members = &groups[&goal];
            // The field pays for itself only when the group's own paths
            // cover the map enough times over; a field the batch holds is
            // held to the same test, so the decision does not depend on what
            // the batch held before.
            if !Self::group_repays_the_build(members, &starts, grid, goal, demand) {
                continue;
            }
            let field = match batch.fields.get(goal, revision) {
                Some(field) => field,
                None => {
                    // A field of its own costs more than the search it saves.
                    if builds >= BUILDS_PER_PASS {
                        continue;
                    }
                    let Some(field) = GoalField::build(grid, goal) else {
                        continue;
                    };
                    builds += 1;
                    counts.build_cells += field.cells() as u64;
                    counts.built += 1;
                    let field = Arc::new(field);
                    batch.fields.put(goal, revision, field.clone());
                    field
                }
            };
            counts.used += 1;
            counts.cells += field.cells() as u64;
            for index in members {
                fields[*index] = Some(field.clone());
            }
        }
        (fields, counts)
    }

    /// Whether the field's build for `goal` repays itself for the demand.
    ///
    /// The build visits every cell of the ground plane once; the searches
    /// it replaces ask the Foot about the eight neighbours of every cell
    /// they expand, and one such ask costs about as much as eight visits of
    /// the plane. So the build pays for itself when the demand's paths cover
    /// the map that many times over. The paths are measured by their
    /// straight length, which is what a search over open ground expands.
    fn group_repays_the_build(
        members: &[usize],
        starts: &[Option<(u16, u16)>],
        grid: &PathGrid,
        goal: (u16, u16),
        demand: &BTreeMap<(u16, u16), u32>,
    ) -> bool {
        let demand = u64::from(demand.get(&goal).copied().unwrap_or(0));
        if demand == 0 || members.is_empty() {
            return false;
        }
        let mut total = 0u64;
        for index in members {
            let Some(start) = starts[*index] else {
                return false;
            };
            total += u64::from(start.0.abs_diff(goal.0).max(start.1.abs_diff(goal.1)));
        }
        let average = total / members.len() as u64;
        let map_cells = u64::from(grid.width()) * u64::from(grid.height());
        demand
            .saturating_mul(average)
            .saturating_mul(FIELD_BUILD_WORTH)
            >= map_cells
    }

    /// The cell `request`'s core search starts from.
    fn request_start_cell(&self, request: &FootPathRequest) -> Option<(u16, u16)> {
        let actor = self.substrate.entities.get(request.entity_id)?;
        Some(
            super::movement_tick::core_search_cells(
                actor,
                request.destination,
                self.resolved_terrain.as_ref(),
            )
            .0,
        )
    }

    /// The key of `request`'s core search toward `goal` as the world stands.
    /// VERA-only: matches a batched answer to its request; the live mover and
    /// world are the source of truth, so an answer serves only an equal key.
    fn foot_search_key(
        &self,
        request: &FootPathRequest,
        goal: DriveCoord,
        rules: &RuleSet,
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
            locomotor: snap.locomotor_kind,
            allow_zone_hierarchy: self.foot_search_allows_hierarchy(
                request,
                self.interner.object_type(actor.type_ref(), rules)?.is_train,
            ),
            threat_coefficient: self.foot_path_threat_coefficient(request.entity_id).bits(),
            owner: actor.owner(),
            veterancy: actor.veterancy.rank(),
            in_playfield: actor.in_playfield(),
            mission: actor.mission.current(),
            nav_com: actor.navigation.nav_com(),
            attack_target: actor.attack_target.target(),
            radio_contacts: actor.seldom.radio_contacts.clone(),
        })
    }
}

#[cfg(all(test, tests_sim_movement))]
#[path = "path_batch_tests.rs"]
mod tests;
