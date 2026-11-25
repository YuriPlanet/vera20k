//! Speculated object turns (VERA-only, sim-scale; docs/sim-thread.md).
//!
//! With the switch on ([`Simulation::speculate_object_turns`]: a match from
//! its start or from a loaded save, its replay, and sim-bench), a live
//! object pass that begins with at least [`SPECULATE_FROM_SLOTS`] Logic
//! slots first runs the turns of the units and infantry likely to change
//! nothing but themselves side by side on the worker pool, each on a
//! private fork of the world as the pass found it (`world::fork`,
//! `util::forked`). The pass then walks its Logic list as before: at each
//! object's slot it applies that object's kept result, or runs the turn
//! there as it always has. A smaller pass runs every turn in the walk.
//!
//! **Which passes.** Besides the smaller ones, the walk runs every turn of a
//! pass in a campaign (each turn reads the local player's shroud,
//! `TechnoClass` vt+0x4A0's prelude), with the idle-sleep check on
//! (sim-bench's `--sleep-check`, which runs each counted visit whole), with
//! overlay receipts pending from the frame's steps before the pass (every
//! turn's commit would publish them), or with a grid whose cell Dummy is not
//! the world's (some tests; every match binds them together). Each reads
//! what a loaded game reads alike: the slot count, the session, receipts the
//! frame's own earlier steps made (its tail takes them all). Smudge dirt,
//! which no save keeps and which may stay from an earlier frame, is
//! published first.
//!
//! **The one deviation.** A kept turn read the world as the pass found it,
//! not as the turns before it in the same pass left it: an object may react
//! one frame later to what another did earlier in the same frame. It applies
//! only from [`SPECULATE_FROM_SLOTS`] slots up, counted as the pass begins:
//! a smaller game plays as with the switch off, and with the switch off the
//! pass is today's, bit for bit.
//!
//! **What is kept.** A turn whose only writes are its own entity and sleep
//! cell (no structural write to the store: `EntityStore::take_fork_writes`),
//! the frame's append-only sound, fire and combat-light channels and the
//! draws of its own random stream (`world::turn_streams`). A fork runs the
//! slot inside its object's stream, as the walk does, and the slot's end
//! drops what it drew: a Scenario stream that is its base's but for those
//! draws (`SimRng::equal_but_for_turn`) was not written, and the fork keeps
//! its copy for its next turn. A turn that wrote any other world field
//! (`fork::SimulationField`, `substrate::SubstrateField`), the Scenario
//! table among them, another object, or a word of the shared cell Dummy
//! (`SharedCellDummy::speculative`), or that failed, is dropped.
//!
//! **When a kept result applies.** At its slot, unless the object was written
//! earlier in the pass ([`EntityStore::written_this_pass`]: handed out for
//! writing, woken, marked or committed) or is gone; then its turn runs there.
//! The result's sounds go into the channel at the slot with the deliveries
//! the turn made between them ([`SoundStep`]), and the end-of-turn commit
//! runs there when the turn made it ([`Simulation::commit_no_fire`]).
//!
//! **Which turns are tried.** Units and infantry not dying that have no
//! NavCom or are moving now, whose mission handler is not due and whose
//! passive block does not act, but for infantry at rest
//! ([`Simulation::infantry_at_rest`]): shared saved state alone, so every
//! process and a loaded game try the same ones. The selection runs inside
//! the shared cell Dummy's private copy, as a turn does: the passive block's
//! gate may look up a cell the map lacks, which stamps the Dummy. A due
//! handler may name a target, which a kept result could not check against
//! the objects turns before it in the pass removed, and an acting passive
//! block takes a batched scan's answer; a unit standing with a NavCom mostly
//! asks for a path, an infantryman at rest may sleep, which the walk counts
//! for almost nothing and a fork would not, and the other objects mostly
//! write the world. They run in the walk.
//!
//! **Lost turns.** A turn on a fork that reaches work only the world itself
//! may do ([`Lost`]: a step into another cell, a cell mark query, lifting or
//! putting down a cell mark, a path search or request, a shot, a removal, a
//! batched scan's answer, a team's latch, a pointer expiry's broadcast, any
//! boundary or callback of the frame, an anim made for a death or taken
//! down, a read of what only this player's game holds) marks itself lost,
//! skips that work and stops at its next checkpoint
//! ([`Simulation::stop_lost_turn`], after the AI visit and after the
//! locomotor's Process). Such work is costly on a fork, which would copy
//! whole the fields it writes, and its result would be dropped anyway.
//!
//! **Per-player inputs.** Peers in a match, and a game loaded from a save,
//! must try and keep the same turns, so no verdict may depend on what only
//! one process holds. What a fork may still read of it, and why its verdict
//! cannot follow:
//! - the frame's boundaries and callbacks (a client lends more than a
//!   headless frame): asking any loses the turn, lent or not
//!   ([`ForkEffects`]); a sound delivery is noted, not asked;
//! - the local house, the selection, the local discovery bytes and a house's
//!   control by this process ([`note_local_read`]): reading any loses the
//!   turn, whatever its value;
//! - the client's voice set: a steady object's counted visit leaves it out
//!   of the question and records the voice probe's visit itself
//!   (`world::steady_ai`), so it only adds a sound to the result;
//! - the flash countdown, which the current player's orders start: a counted
//!   visit steps it, so steadiness does not ask it, and only the sleep cell a
//!   turn leaves follows it;
//! - idle sleeping's switch (`VERA20K_IDLE_SLEEP=0`): a fork marks quiet and
//!   steady whatever the switch (see **Sleep**), which then decides only the
//!   sleep cell a turn leaves, read by no verdict.
//!
//! The results' sounds and sleep cells may differ between peers, as the
//! walk's do; neither is hashed or decides a verdict.
//!
//! **Cost.** A turn on a fork costs more than the same turn in the walk: its
//! first write copies its object's main record (the seldom part stays shared
//! until written, [`crate::sim::game_entity::SeldomPart`]), and a lost or
//! dropped turn runs again at its slot. The results must not depend on the
//! thread count, so every build pays this and only threads hide it. In
//! sim-bench's battle (30 houses, about 23,000 objects, 1,200 frames, an
//! Apple M4 busy with other work, each run started beside one with
//! `--serial-turns`) a pass of 26,200 slots tried 13,800 turns and applied
//! 12,400 of them. The object pass took 13 to 14% longer with one worker
//! (`--threads 1`, beside which the simulation thread runs forks too), 3 to
//! 5% less with four and 9% less with the whole pool. A build without a
//! pool runs every tried turn on its one thread before the walk: 51 to 52%
//! longer when last measured, on a build whose every fork ran on one thread
//! and which still tried infantry at rest.
//! [`SPECULATE_FROM_SLOTS`] bounds that cost: a game whose passes begin with
//! fewer Logic slots pays none of it.
//!
//! **Memory.** A turn writes its copy into a record allocation an earlier
//! pass left (`Speculation::spares`: records kept results replaced, copies
//! no result applied, one a slot), not a new one. A new copy for every tried
//! turn, allocated on the workers and freed on other threads, left the
//! allocator's free room spread over every thread's share of it: with the
//! whole pool a run peaked 0.7 to 2.1 GB above its `--serial-turns` twin
//! with the same live bytes (macOS's allocator held 3.0 GB for 435 MB of
//! allocations, against 0.79 GB for 428 MB). With the spares the live bytes
//! are the twin's (460 MB in the battle), but the footprint is not: with
//! four or all workers it rises to about 1.0 GB and falls back, and peaks
//! 0.4 to 0.5 GB above the twin's 0.51 to 0.57 GB (70 to 90 MB with one
//! worker). The extra is free room the allocator keeps in its small
//! regions (44% of them, against 20%); its cause is not yet found.
//!
//! **Sleep.** A fork first wakes its own object, settling the visits it is
//! owed, then marks it quiet and steady again from that state alone
//! ([`Simulation::quiet_after_turn`], [`Simulation::steady_after_turn`]),
//! whatever this process's sleep switch, and runs its turn, which skips and
//! counts what the walk's would: a steady object's AI visit is counted on
//! the fork, which notes it for the pass to count where the result applies.
//! No turn reads the marks the world holds for its object, which no save
//! keeps, so a loaded game keeps the same turns. The kept result leaves the
//! state hash a counted visit would; with the switch off, its commit clears
//! the marks the fork left. The infantry that may sleep are not tried.
//!
//! **A fork stays its base.** After every turn each written field takes its
//! base's handle again ([`Simulation::restore_written_fields_from`]), the
//! store drops its writes and touch logs (`EntityStore::reset_fork`), the
//! Dummy's private word goes with its run, and this thread's turn notes are
//! put back. Debug builds run each object's kept turn again on a fresh fork
//! once in sixteen frames and require the same result.
//!
//! **Determinism.** Each fork turn reads the pass-start world alone and
//! draws from its own object's stream, so its verdict does not depend on
//! threads or on which turns a thread ran before; the walk applies results
//! in Logic order, and their validity reads the stamps the walk itself
//! writes.

use std::cell::Cell;
use std::sync::Arc;
use std::time::Duration;

use super::fork::SimulationField;
use super::frame_channel::FrameChannel;
use super::frame_effects::{ForkEffects, SoundStep};
use super::object_turn::ObjectTurnOutcome;
use super::substrate::{ObjectSubstrate, SubstrateField};
use super::{FrameAdvanceError, FrameEffects, SimFireEvent, SimSoundEvent, Simulation};
use crate::map::entities::EntityCategory;
use crate::map::resolved_terrain::SharedCellDummy;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::CombatLightRequest;
use crate::sim::entity_store::EntityWrites;
use crate::sim::game_entity::GameEntity;
use crate::sim::movement::SpeedRules;
use crate::util::forked::Forked;
use crate::util::lookup_hash::LookupMap;
use crate::util::time::Instant;

/// The world fields a kept turn may have written: the append-only channels,
/// whose facts the pass appends at the turn's slot.
const MERGEABLE_FIELDS: u128 = (1 << SimulationField::sound_events as u32)
    | (1 << SimulationField::fire_events as u32)
    | (1 << SimulationField::combat_light_requests as u32);

/// The substrate's entity store, whose writes the pass merges itself.
const ENTITIES: u128 = 1 << SubstrateField::entities as u32;

/// The fewest Logic slots a pass must begin with to run turns on forks. A
/// smaller pass runs every turn in the walk, as with the switch off, however
/// many objects its turns make or remove: the slot count is saved state, so
/// every peer, thread count and platform decides alike. A pass of a few
/// hundred objects takes well under a millisecond, where forks buy nothing
/// but the deviation. In sim-bench's battle on smaller maps of its kind (30
/// houses, the whole pool, each run beside one with `--serial-turns`), the
/// object pass of about 1,600 slots took 2% less and 4% more, of 3,000 slots
/// 7% and 8% less and of 5,300 slots 9% less: the smallest size measured no
/// slower, rounded up to a power of two.
const SPECULATE_FROM_SLOTS: usize = 4096;

/// Debug builds run each object's kept turn again on a fresh fork once in
/// this many frames.
#[cfg(debug_assertions)]
const LEAK_CHECK_EVERY: u64 = 16;

/// Why a turn run on a fork was lost: it reached work only the world itself
/// may do. The turn skips that work and stops at its next checkpoint, and
/// the pass runs it at its slot. Each is work whose writes no kept result
/// may hold, and most of it writes a large field, which a fork would copy
/// whole: the map's cells, the occupancy, the anims, the teams.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Lost {
    /// A step into another cell: the per-cell Process, the walk's completed
    /// step and boundary, the track's crossing and occupation hand-off.
    CellStep,
    /// The track's cell mark query.
    MarkQuery,
    /// An object's cell mark lifted from the map or put down on it
    /// (`MapClass::Pick_Up`, `Place_Down`).
    Marks,
    /// A path search, or a request deferred to the path batch.
    Path,
    /// A shot (`TechnoClass::Fire_At`'s launch).
    Fire,
    /// An object's removal from the world (`UnInit`).
    Removal,
    /// A batched target scan's answer taken.
    Scan,
    /// A team's leaving-map latch set.
    Team,
    /// A pointer expiry's broadcast to its listeners.
    Expiry,
    /// A boundary or callback of the frame asked, whether or not the frame
    /// lends it.
    Boundary,
    /// An Anim made for a death or taken down (`AnimClass` ctor, `UnInit`).
    Anim,
    /// A read of what only this player's game holds ([`note_local_read`]).
    Local,
}

impl Lost {
    const ALL: [Lost; 12] = [
        Lost::CellStep,
        Lost::MarkQuery,
        Lost::Marks,
        Lost::Path,
        Lost::Fire,
        Lost::Removal,
        Lost::Scan,
        Lost::Team,
        Lost::Expiry,
        Lost::Boundary,
        Lost::Anim,
        Lost::Local,
    ];

    fn name(self) -> &'static str {
        match self {
            Lost::CellStep => "cell_step",
            Lost::MarkQuery => "mark_query",
            Lost::Marks => "marks",
            Lost::Path => "path",
            Lost::Fire => "fire",
            Lost::Removal => "removal",
            Lost::Scan => "scan",
            Lost::Team => "team",
            Lost::Expiry => "expiry",
            Lost::Boundary => "boundary",
            Lost::Anim => "anim",
            Lost::Local => "local",
        }
    }
}

/// What the turn a thread runs on a fork noted about itself.
#[derive(Debug, Clone, Copy, Default)]
struct Notes {
    /// The first reason it was lost.
    lost: Option<Lost>,
    /// It made the end-of-turn commit.
    committed: bool,
    /// It counted its steady object's AI visit (`world::steady_ai`).
    steady_visit: bool,
}

thread_local! {
    /// The notes of the turn this thread runs on a fork of the world, while
    /// it runs one.
    static TURN: Cell<Option<Notes>> = const { Cell::new(None) };
}

/// One fork turn's notes on this thread. A worker may take up another turn
/// while it waits inside one (rayon steals work), so the guard puts back the
/// notes it found.
struct TurnNotes {
    outer: Option<Notes>,
}

impl TurnNotes {
    fn begin() -> Self {
        Self {
            outer: TURN.replace(Some(Notes::default())),
        }
    }

    /// The turn's notes, as it ends.
    fn end(self) -> Notes {
        TURN.get().unwrap_or_default()
    }
}

impl Drop for TurnNotes {
    fn drop(&mut self) {
        TURN.set(self.outer.take());
    }
}

/// Mark the turn this thread runs on a fork lost, unless it already is.
/// Every caller holds the fork by `&mut`, so the mark is made on the turn's
/// own thread.
pub(super) fn mark_lost(at: Lost) {
    if let Some(notes) = TURN.get()
        && notes.lost.is_none()
    {
        TURN.set(Some(Notes {
            lost: Some(at),
            ..notes
        }));
    }
}

/// A read of what only this player's game holds: the local house, the
/// selection, the local discovery bytes, a house's control by this process
/// (see the module's **Per-player inputs**). A turn on a fork that makes one
/// is lost, so its verdict cannot depend on the value read; outside such a
/// turn, nothing.
pub(crate) fn note_local_read() {
    mark_lost(Lost::Local);
}

/// Note that the turn this thread runs on a fork made the end-of-turn commit,
/// which the pass makes at the turn's slot.
pub(super) fn note_commit() {
    if let Some(notes) = TURN.get() {
        TURN.set(Some(Notes {
            committed: true,
            ..notes
        }));
    }
}

/// Note that the turn this thread runs on a fork counted its steady object's
/// AI visit, which the pass counts where the turn's result applies; false
/// outside a turn on a fork, whose caller counts it itself.
pub(super) fn note_steady_visit() -> bool {
    let Some(notes) = TURN.get() else {
        return false;
    };
    TURN.set(Some(Notes {
        steady_visit: true,
        ..notes
    }));
    true
}

/// The switch, the pass size it starts from, its counters, and the room a
/// pass reuses. Never saved or hashed.
pub(crate) struct Speculation {
    on: bool,
    /// [`SPECULATE_FROM_SLOTS`]; tests of small worlds lower it.
    from_slots: usize,
    stats: ObjectPassStats,
    /// The last pass's slot list, emptied, for its room.
    slots: Vec<Slot>,
    /// Record allocations the next pass's turns write their copies into
    /// ([`EntityStore::lend_record`]): the records kept results replaced and
    /// the copies no result applied, no more than one per slot (see the
    /// module's **Memory**). Each holds a stale record that nothing reads.
    ///
    /// [`EntityStore::lend_record`]: crate::sim::entity_store::EntityStore::lend_record
    spares: Vec<Arc<GameEntity>>,
}

impl Default for Speculation {
    fn default() -> Self {
        Self {
            on: false,
            from_slots: SPECULATE_FROM_SLOTS,
            stats: ObjectPassStats::default(),
            slots: Vec::new(),
            spares: Vec::new(),
        }
    }
}

/// A clone keeps the switch and the counters; the room is the world's own.
impl Clone for Speculation {
    fn clone(&self) -> Self {
        Self {
            on: self.on,
            from_slots: self.from_slots,
            stats: self.stats.clone(),
            ..Self::default()
        }
    }
}

impl std::fmt::Debug for Speculation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Speculation")
            .field("on", &self.on)
            .field("from_slots", &self.from_slots)
            .field("stats", &self.stats)
            .field("spares", &self.spares.len())
            .finish_non_exhaustive()
    }
}

/// What the live object pass did with speculated turns, summed over its
/// passes: sim-bench prints it as its `object_pass` line.
#[derive(Debug, Clone)]
pub struct ObjectPassStats {
    /// Passes that ran turns on forks.
    pub passes: u64,
    /// Passes with the switch on that ran every turn in the walk: the pass
    /// began with fewer Logic slots than [`SPECULATE_FROM_SLOTS`], the game
    /// is a campaign, the idle-sleep check was on, a commit was pending, or
    /// the grid had a Dummy of its own.
    pub unspeculated: u64,
    /// Logic slots the walks of those passes reached.
    pub walked: u64,
    /// Turns run on a fork.
    pub speculated: u64,
    /// Turns whose results were kept.
    pub kept: u64,
    /// Kept results applied at their slots.
    pub committed: u64,
    /// Kept results dropped at their slots: an earlier turn of the pass had
    /// written the object, or it was gone.
    pub kept_invalid: u64,
    /// Kept results whose slots the walk did not reach.
    pub unvisited: u64,
    /// Turns not kept for writing a world field other than the channels.
    pub fields: u64,
    /// Turns not kept for a structural write to the entity store.
    pub structural: u64,
    /// Turns not kept for writing another object or its sleep cell.
    pub other_entity: u64,
    /// Turns not kept for stamping or writing the shared cell Dummy.
    pub cell_dummy: u64,
    /// Turns not kept because they failed.
    pub errors: u64,
    /// Turns lost, by [`Lost`].
    lost: [u64; Lost::ALL.len()],
    /// How often each world field, then each substrate field, kept a turn
    /// from being kept: by its `written_mask` bit.
    world_fields: [u64; 128],
    substrate_fields: [u64; 128],
    /// Passes with the switch off.
    pub serial_passes: u64,
    /// Copies of a record a turn on a fork made in a new allocation rather
    /// than a spare one.
    pub new_records: u64,
    speculate_time: Duration,
    walk_time: Duration,
    /// Every pass's time, from the speculation's start or the walk's, with
    /// the switch on or off.
    pass_time: Duration,
}

impl Default for ObjectPassStats {
    fn default() -> Self {
        Self {
            passes: 0,
            unspeculated: 0,
            walked: 0,
            speculated: 0,
            kept: 0,
            committed: 0,
            kept_invalid: 0,
            unvisited: 0,
            fields: 0,
            structural: 0,
            other_entity: 0,
            cell_dummy: 0,
            errors: 0,
            lost: [0; Lost::ALL.len()],
            world_fields: [0; 128],
            substrate_fields: [0; 128],
            serial_passes: 0,
            new_records: 0,
            speculate_time: Duration::ZERO,
            walk_time: Duration::ZERO,
            pass_time: Duration::ZERO,
        }
    }
}

impl ObjectPassStats {
    /// Turns lost for `at`.
    #[cfg(test)]
    pub(crate) fn lost(&self, at: Lost) -> u64 {
        self.lost[at as usize]
    }

    /// Count a turn's verdict.
    fn count(&mut self, verdict: &Verdict) {
        if !matches!(verdict, Verdict::Untried) {
            self.speculated += 1;
        }
        match verdict {
            Verdict::Untried => {}
            Verdict::Kept => self.kept += 1,
            Verdict::Lost(at) => self.lost[*at as usize] += 1,
            Verdict::Fields { world, substrate } => {
                self.fields += 1;
                for (fields, mask) in [
                    (&mut self.world_fields, *world),
                    (&mut self.substrate_fields, *substrate),
                ] {
                    for (bit, count) in fields.iter_mut().enumerate() {
                        if mask & (1 << bit) != 0 {
                            *count += 1;
                        }
                    }
                }
            }
            Verdict::Structural => self.structural += 1,
            Verdict::OtherEntity => self.other_entity += 1,
            Verdict::CellDummy => self.cell_dummy += 1,
            Verdict::Error => self.errors += 1,
        }
    }
}

/// One line: the counters, the lost turns by kind, the eight fields that
/// most often kept a turn from being kept, and the mean time per pass of
/// the speculation and of the walk after it.
impl std::fmt::Display for ObjectPassStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "object_pass passes={} unspeculated={} walked={} speculated={} kept={} \
             committed={} kept_invalid={} unvisited={} fields={} structural={} \
             other_entity={} cell_dummy={} errors={}",
            self.passes,
            self.unspeculated,
            self.walked,
            self.speculated,
            self.kept,
            self.committed,
            self.kept_invalid,
            self.unvisited,
            self.fields,
            self.structural,
            self.other_entity,
            self.cell_dummy,
            self.errors,
        )?;
        for at in Lost::ALL {
            write!(f, " lost_{}={}", at.name(), self.lost[at as usize])?;
        }
        let named = |fields: &[u64; 128], name: fn(u32) -> &'static str| {
            (0..128)
                .filter(|&bit| fields[bit as usize] > 0)
                .map(move |bit| (name(bit), fields[bit as usize]))
                .collect::<Vec<_>>()
        };
        let mut top = named(&self.world_fields, Simulation::field_name);
        top.extend(named(&self.substrate_fields, ObjectSubstrate::field_name));
        top.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        write!(f, " top_fields=")?;
        if top.is_empty() {
            write!(f, "-")?;
        }
        for (index, (name, count)) in top.iter().take(8).enumerate() {
            let comma = if index == 0 { "" } else { "," };
            write!(f, "{comma}{name}:{count}")?;
        }
        let per_pass = |time: Duration| {
            if self.passes == 0 {
                0.0
            } else {
                time.as_secs_f64() * 1000.0 / self.passes as f64
            }
        };
        let all = self.passes + self.unspeculated + self.serial_passes;
        let pass_ms = if all == 0 {
            0.0
        } else {
            self.pass_time.as_secs_f64() * 1000.0 / all as f64
        };
        write!(
            f,
            " speculate_ms={:.2} walk_ms={:.2} pass_ms={pass_ms:.2} record_bytes={} \
             new_records={}",
            per_pass(self.speculate_time),
            per_pass(self.walk_time),
            std::mem::size_of::<GameEntity>(),
            self.new_records
        )
    }
}

/// What a turn on a fork came to.
#[derive(Debug)]
enum Verdict {
    /// The pass did not try it ([`likely_kept`]).
    Untried,
    /// Its result is kept for its slot ([`Slot::kept`]).
    Kept,
    Lost(Lost),
    /// It wrote these world and substrate fields beside the mergeable ones.
    Fields {
        world: u128,
        substrate: u128,
    },
    Structural,
    OtherEntity,
    CellDummy,
    Error,
}

/// A turn's result, kept for its slot.
#[derive(Debug)]
struct Kept {
    writes: EntityWrites,
    outcome: ObjectTurnOutcome,
    sounds: Vec<SimSoundEvent>,
    fires: Vec<SimFireEvent>,
    lights: Vec<CombatLightRequest>,
    sound_steps: Vec<SoundStep>,
    /// The turn made the end-of-turn commit.
    committed: bool,
    /// The turn counted its steady object's AI visit.
    steady_visit: bool,
}

/// What every turn of a pass is run with.
struct TurnInputs<'a> {
    rules: &'a RuleSet,
    overlay_registry: Option<&'a OverlayTypeRegistry>,
    dummy: SharedCellDummy,
    selection_voice_suppressed: bool,
}

/// One thread's fork of the world: made for its first turn of a pass, and
/// returned to its base after each.
#[derive(Default)]
struct Worker {
    fork: Option<Box<Simulation>>,
}

/// Every thread makes a fork of its own.
impl Clone for Worker {
    fn clone(&self) -> Self {
        Self::default()
    }
}

impl Worker {
    /// `slot`'s turn on this thread's fork, which writes its copy of the
    /// object into the slot's spare record, if it has one. The slot keeps
    /// that record if the copy did not take it, or else the copy if the turn
    /// was dropped, for the pass's end to return.
    fn turn(&mut self, world: &Simulation, slot: &mut Slot, inputs: &TurnInputs<'_>) {
        let fork = self.fork.get_or_insert_with(|| Box::new(world.fork()));
        fork.substrate.entities.lend_record(slot.spare.take());
        let (verdict, kept) = fork.speculate_turn(world, slot.id, inputs);
        (slot.spare, slot.new_records) = fork.substrate.entities.take_record();
        #[cfg(debug_assertions)]
        if (slot.id + u64::from(world.session.binary_frame())).is_multiple_of(LEAK_CHECK_EVERY)
            && let Some(kept) = &kept
        {
            let mut fresh = Box::new(world.fork());
            let again = match fresh.speculate_turn(world, slot.id, inputs) {
                (_, Some(again)) => format!("{again:?}"),
                (verdict, None) => format!("{verdict:?}"),
            };
            assert_eq!(
                format!("{kept:?}"),
                again,
                "object {}'s turn on a fork that ran turns before differs from its turn \
                 on a fresh fork: a turn left something behind",
                slot.id
            );
        }
        slot.verdict = verdict;
        slot.kept = kept;
    }
}

/// One Logic slot of a speculated pass.
#[derive(Debug)]
struct Slot {
    id: u64,
    /// A spare record for the slot's turn to write its copy into; after the
    /// turn, the spare it did not write into or its copy from a dropped
    /// turn, until the pass's end returns it to the spares.
    spare: Option<Arc<GameEntity>>,
    verdict: Verdict,
    /// The turn's result, kept for the walk.
    kept: Option<Kept>,
    /// The turn's copies that took a new allocation.
    new_records: u64,
}

impl Slot {
    fn new(id: u64, spare: Option<Arc<GameEntity>>) -> Self {
        Self {
            id,
            spare,
            verdict: Verdict::Untried,
            kept: None,
            new_records: 0,
        }
    }
}

/// When the live object pass began ([`Simulation::count_object_pass`]).
pub(super) struct PassClock(Instant);

impl PassClock {
    pub(super) fn start() -> Self {
        Self(Instant::now())
    }
}

/// One pass's speculated turns, for its walk.
pub(super) struct SpeculatedPass {
    /// Each Logic slot as the pass began, in order, with its kept result.
    slots: Vec<Slot>,
    /// The slot the walk should reach next.
    next: usize,
    /// The slots by id, made when the walk first leaves their order.
    positions: Option<LookupMap<u64, usize>>,
    /// The spares for the next pass: those no slot took, the entities kept
    /// results replaced and the copies of results dropped.
    spares: Vec<Arc<GameEntity>>,
    walked: u64,
    committed: u64,
    kept_invalid: u64,
    walk_started: Instant,
}

impl SpeculatedPass {
    /// The kept result of the slot the walk reached with `id`, if any. An
    /// object that joined or left Logic since the pass began puts the walk
    /// out of the slots' order; it then finds `id`'s slot by id, past the
    /// last one it reached.
    fn take(&mut self, id: u64) -> Option<Kept> {
        let at = if self.slots.get(self.next).is_some_and(|slot| slot.id == id) {
            self.next
        } else {
            let positions = self.positions.get_or_insert_with(|| {
                let mut positions = LookupMap::default();
                for (at, slot) in self.slots.iter().enumerate() {
                    positions.entry(slot.id).or_insert(at);
                }
                positions
            });
            let at = *positions.get(&id)?;
            if at < self.next {
                return None;
            }
            at
        };
        self.next = at + 1;
        self.slots[at].kept.take()
    }
}

/// Whether `id`'s turn is worth trying on a fork: a unit's or infantryman's,
/// not dying, that has no NavCom or is moving now, whose mission handler is
/// not due and whose passive block does not act, and not an infantryman at
/// rest. Shared saved state alone: run inside the shared cell Dummy's
/// private copy, as the passive block's gate may look a missing cell up.
fn likely_kept(
    world: &Simulation,
    id: u64,
    rules: &RuleSet,
    speed: SpeedRules<'_>,
    frame: u32,
) -> bool {
    world.substrate.entities.get(id).is_some_and(|entity| {
        let category = entity.category();
        matches!(category, EntityCategory::Unit | EntityCategory::Infantry)
            && !entity.death.is_dying()
            && (entity.navigation.nav_com().is_none()
                || crate::sim::movement::motion_query::is_moving_now(entity, speed, frame))
            // A due handler mostly draws the Scenario stream for its next
            // delay (`mission::authority::rate_epilogue`), and a passive
            // block that acts as its targeting timer is out scans and draws
            // it too; one whose gate refuses leaves the timer out
            // (`idle_sleep` asks the same).
            && !entity.mission_handler_due(frame)
            && !(entity.passive_scan.timer().due(frame)
                && super::techno_ai::passive_block_acts(world, id, rules))
            // An infantryman at rest may sleep, and the walk then counts its
            // visit for almost nothing; a fork would wake it and run its
            // turn whole, every frame (`world::idle_sleep`).
            && !(category == EntityCategory::Infantry && world.infantry_at_rest(id, rules))
    })
}

/// A fork's facts of one channel: its turn's alone, as the channel was
/// empty when the world was forked.
fn take_facts<T: Clone>(channel: &mut Forked<FrameChannel<T>>) -> Vec<T> {
    if Forked::written(channel) {
        channel.take()
    } else {
        Vec::new()
    }
}

impl Simulation {
    /// Run the turns likely to change nothing but their own object on forks
    /// of the world first, side by side, from now on, or stop (see the module
    /// docs). Sim-scale: a match turns it on as it starts or is loaded, as
    /// its replay and sim-bench do; off, the pass is today's. Not saved: a
    /// save carries no switch, and the loader turns it on.
    pub fn speculate_object_turns(&mut self, on: bool) {
        self.speculation.on = on;
    }

    /// Run turns on forks from passes of `slots` Logic slots up, for a
    /// test's world smaller than [`SPECULATE_FROM_SLOTS`].
    #[cfg(test)]
    pub(crate) fn speculate_from_slots_for_test(&mut self, slots: usize) {
        self.speculation.from_slots = slots;
    }

    /// What the object pass did with speculated turns so far.
    pub fn object_pass_stats(&self) -> &ObjectPassStats {
        &self.speculation.stats
    }

    /// Whether this world is a fork running an object turn, which must not
    /// do the work it is about to (`at`): the turn is then marked lost.
    pub(crate) fn lose_fork_turn(&self, at: Lost) -> bool {
        if !self.substrate.entities.is_fork() {
            return false;
        }
        mark_lost(at);
        true
    }

    /// The error a lost turn on a fork stops with; nothing reports it.
    pub(crate) fn lost_turn_error(&self, id: u64) -> FrameAdvanceError {
        FrameAdvanceError::new(
            self.session.tick(),
            self.session.binary_frame(),
            id,
            String::new(),
        )
    }

    /// A checkpoint of `id`'s turn: one on a fork that was lost stops here.
    pub(super) fn stop_lost_turn(&self, id: u64) -> Result<(), FrameAdvanceError> {
        if TURN.get().is_some_and(|notes| notes.lost.is_some()) {
            return Err(self.lost_turn_error(id));
        }
        Ok(())
    }

    /// The start of the live object pass's walk, with the switch on and at
    /// least [`SPECULATE_FROM_SLOTS`] Logic slots: run the tried turns on
    /// forks of the world as it is now, side by side, and keep the results
    /// the walk may apply ([`Self::commit_kept_turn`]). The world is
    /// unchanged, but for the store's pass stamps
    /// ([`crate::sim::entity_store::EntityStore::begin_pass`]). `None` runs
    /// every turn of the pass in the walk.
    pub(super) fn speculate_live_object_turns(
        &mut self,
        rules: &RuleSet,
        overlay_registry: Option<&OverlayTypeRegistry>,
        effects: &mut FrameEffects<'_>,
    ) -> Option<SpeculatedPass> {
        if !self.speculation.on {
            return None;
        }
        // Counted once, as the pass begins: what its turns make or remove
        // does not change how it runs.
        if self.substrate.logic.len() < self.speculation.from_slots {
            self.speculation.stats.unspeculated += 1;
            return None;
        }
        // The smudge grid's dirt waits for the next commit, the walk's first
        // turn's, and may stay from an earlier frame; no save keeps it, so
        // publish it now (the radar's redraw marks) and let the gate read
        // what a loaded game reads too.
        Self::flush_smudge_dirty(&mut self.smudge_grid, &mut self.dirty_cells);
        // The check runs every counted visit, and a campaign's turns read the
        // local player's shroud (`TechnoClass` vt+0x4A0's prelude). A turn's
        // fork must reach the Dummy its run detaches: in every match the
        // grid's is the world's (`bind_shared_cell_dummy`). Overlay receipts
        // pending from this frame's steps before the pass (the frame's tail
        // takes them all) would make every turn's commit write the world.
        let dummy = self.effective_shared_cell_dummy();
        if self.idle_sleep.check
            || self.session.uses_local_player_shroud()
            || self.commit_work_pending()
            || !dummy.same_identity(&self.shared_cell_dummy)
        {
            self.speculation.stats.unspeculated += 1;
            return None;
        }
        let started = Instant::now();
        let inputs = TurnInputs {
            rules,
            overlay_registry,
            dummy,
            selection_voice_suppressed: !effects.selection_voice_enabled(),
        };
        // Each slot takes a spare record for its turn's copy, while they last.
        let mut spares = std::mem::take(&mut self.speculation.spares);
        let mut slots = std::mem::take(&mut self.speculation.slots);
        slots.extend(
            self.substrate
                .logic
                .as_slice()
                .iter()
                .map(|&id| Slot::new(id, spares.pop())),
        );
        self.substrate.entities.begin_pass();
        // The frame's facts so far wait outside the world while it is forked,
        // so that a fork's channels hold its own turn's facts alone.
        let sounds = self.sound_events.take();
        let fires = self.fire_events.take();
        let lights = self.combat_light_requests.take();
        self.share_fields();
        {
            let world = &*self;
            let speed = SpeedRules::new(rules, &world.interner, &world.houses);
            let frame = world.session.binary_frame();
            crate::sim::parallel::for_each_mut_with(
                &mut slots,
                Worker::default(),
                |worker, slot| {
                    let (tried, _) = inputs
                        .dummy
                        .speculative(|| likely_kept(world, slot.id, inputs.rules, speed, frame));
                    if tried {
                        worker.turn(world, slot, &inputs);
                    }
                },
            );
        }
        self.unshare_fields();
        self.sound_events.put_back(sounds);
        self.fire_events.put_back(fires);
        self.combat_light_requests.put_back(lights);
        let stats = &mut self.speculation.stats;
        for slot in &slots {
            stats.count(&slot.verdict);
            stats.new_records += slot.new_records;
        }
        stats.passes += 1;
        stats.speculate_time += started.elapsed();
        Some(SpeculatedPass {
            slots,
            next: 0,
            positions: None,
            spares,
            walked: 0,
            committed: 0,
            kept_invalid: 0,
            walk_started: Instant::now(),
        })
    }

    /// The walk reached `id`'s slot: apply its kept result, if it has one
    /// and no turn before it in the pass wrote the object, and answer the
    /// turn's outcome; `None` runs the turn here.
    pub(super) fn commit_kept_turn(
        &mut self,
        id: u64,
        pass: &mut SpeculatedPass,
        rules: &RuleSet,
        effects: &mut FrameEffects<'_>,
    ) -> Option<ObjectTurnOutcome> {
        pass.walked += 1;
        let kept = pass.take(id)?;
        if !self.substrate.entities.contains(id) || self.substrate.entities.written_this_pass(id) {
            pass.kept_invalid += 1;
            pass.spares.extend(kept.writes.into_records());
            return None;
        }
        pass.committed += 1;
        let Kept {
            writes,
            mut outcome,
            sounds,
            fires,
            lights,
            sound_steps,
            committed,
            steady_visit,
        } = kept;
        self.substrate
            .entities
            .commit_writes(writes, &mut pass.spares);
        // A fork marks its object quiet and steady whatever this process's
        // sleep switch; with the switch off the world holds no marks.
        if !self.idle_sleep.is_on() {
            self.substrate.entities.wake(id);
        }
        self.idle_sleep.steady += u64::from(steady_visit);
        // Each sound delivery the turn made, after the sounds it had
        // emitted by then.
        let mut sounds = sounds.into_iter();
        let mut appended = 0;
        for step in sound_steps {
            self.sound_events
                .emit_all(sounds.by_ref().take(step.emitted.saturating_sub(appended)));
            appended = step.emitted;
            match step.looping {
                None => effects.flush_sound(self, rules),
                Some((owner, world)) => effects.update_looping_sound(self, rules, owner, world),
            }
        }
        self.sound_events.emit_all(sounds);
        self.fire_events.emit_all(fires);
        self.combat_light_requests.emit_all(lights);
        if committed {
            outcome.add_commit(self.commit_no_fire(rules, effects));
        }
        Some(outcome)
    }

    /// The live object pass is over, with the switch on or off: count its
    /// time from `clock` for sim-bench's `pass_ms`.
    pub(super) fn count_object_pass(&mut self, clock: PassClock) {
        let serial = !self.speculation.on;
        let stats = &mut self.speculation.stats;
        stats.serial_passes += u64::from(serial);
        stats.pass_time += clock.0.elapsed();
    }

    /// The walk is over: count it, and keep as the next pass's spares the
    /// entities kept results replaced, the copies of results it did not
    /// apply and the spares no copy took, at most one a slot.
    pub(super) fn finish_speculated_pass(&mut self, pass: SpeculatedPass) {
        let SpeculatedPass {
            mut slots,
            mut spares,
            walked,
            committed,
            kept_invalid,
            walk_started,
            ..
        } = pass;
        let mut unvisited = 0;
        for slot in &mut slots {
            spares.extend(slot.spare.take());
            if let Some(kept) = slot.kept.take() {
                unvisited += 1;
                spares.extend(kept.writes.into_records());
            }
        }
        if spares.len() > slots.len() {
            crate::sim::parallel::drop_on_pool(spares.split_off(slots.len()));
        }
        slots.clear();
        let speculation = &mut *self.speculation;
        let stats = &mut speculation.stats;
        stats.walked += walked;
        stats.committed += committed;
        stats.kept_invalid += kept_invalid;
        stats.unvisited += unvisited;
        stats.walk_time += walk_started.elapsed();
        speculation.slots = slots;
        speculation.spares = spares;
    }

    /// `id`'s turn on this fork of `world` ([`Self::fork`]), and the fork
    /// back to `world` after it: what it came to, and its result if kept.
    fn speculate_turn(
        &mut self,
        world: &Simulation,
        id: u64,
        inputs: &TurnInputs<'_>,
    ) -> (Verdict, Option<Kept>) {
        let notes = TurnNotes::begin();
        let mut effects = ForkEffects::new(inputs.selection_voice_suppressed);
        let (ran, dummy_written) = inputs.dummy.speculative(|| {
            // The slot draws from its object's own stream, as it does in the
            // walk (`world::turn_streams`).
            self.enter_turn_stream(id);
            // A sleeping object takes its turn from its settled state, and
            // the object's quiet and steady marks come from that state again,
            // not from the world's: its turn skips and counts what the walk's
            // would, and a loaded game, whose objects start unmarked, keeps
            // the same turns.
            self.substrate.entities.focus(id);
            self.substrate.entities.wake(id);
            self.quiet_after_turn(id, inputs.rules);
            self.steady_after_turn(id, inputs.rules);
            let mut object_ctx = super::techno_ai::ObjectAiCtx {
                effects: FrameEffects::for_fork(&mut effects),
                overlay_registry: inputs.overlay_registry,
            };
            let ran = self.run_live_object_slot(id, inputs.rules, &mut object_ctx);
            self.leave_turn_stream(id);
            ran
        });
        let notes = notes.end();
        // The slot's end dropped what its own stream drew: a Scenario stream
        // that is its base's but for that was not written, and the fork
        // keeps its copy for its next turn instead of copying the table
        // again.
        if Forked::written(&self.scenario_rng)
            && self.scenario_rng.equal_but_for_turn(&world.scenario_rng)
        {
            Forked::clear_written(&mut self.scenario_rng);
        }
        let verdict = self.verdict(id, ran, dummy_written, notes, effects.sound_steps);
        self.restore_written_fields_from(world);
        debug_assert_eq!(
            self.written_mask() | self.substrate.written_mask(),
            0,
            "a fork is its base again after a turn"
        );
        match verdict {
            Ok(kept) => (Verdict::Kept, Some(kept)),
            Err(verdict) => (verdict, None),
        }
    }

    /// What `id`'s turn on this fork came to: a kept result, which takes the
    /// fork's writes and facts, or why it is not kept.
    fn verdict(
        &mut self,
        id: u64,
        ran: Result<Option<ObjectTurnOutcome>, FrameAdvanceError>,
        dummy_written: bool,
        notes: Notes,
        sound_steps: Vec<SoundStep>,
    ) -> Result<Kept, Verdict> {
        if let Some(at) = notes.lost {
            return Err(Verdict::Lost(at));
        }
        // A fork wakes its object, so its slot always runs the turn.
        let Ok(Some(outcome)) = ran else {
            return Err(Verdict::Error);
        };
        if dummy_written {
            return Err(Verdict::CellDummy);
        }
        let world = self.written_mask() & !MERGEABLE_FIELDS;
        let substrate = self.substrate.written_mask() & !ENTITIES;
        if world | substrate != 0 {
            return Err(Verdict::Fields { world, substrate });
        }
        let writes = self.substrate.entities.take_fork_writes();
        if writes.structural() {
            return Err(Verdict::Structural);
        }
        if writes.ids().any(|written| written != id) {
            return Err(Verdict::OtherEntity);
        }
        Ok(Kept {
            writes,
            outcome,
            sounds: take_facts(&mut self.sound_events),
            fires: take_facts(&mut self.fire_events),
            lights: take_facts(&mut self.combat_light_requests),
            sound_steps,
            committed: notes.committed,
            steady_visit: notes.steady_visit,
        })
    }
}
