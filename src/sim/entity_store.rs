//! Dense entity storage with deterministic sorted iteration.
//!
//! `EntityStore` holds every game entity, behind an `Arc`, in ascending
//! stable_id order in one vector, with an id -> slot index for O(1) lookup. Stable ids are
//! monotonic and never reused, so a new entity normally appends; a removed one
//! leaves an empty slot until the next compaction, which keeps the order.
//! Iteration is always in ascending stable_id order, which lockstep relies on.
//!
//! ## Borrow patterns
//! - Callers outside simulation borrow stored entities through `get` or the
//!   ordered iterators. Store operations maintain the indexes when an entity
//!   enters, leaves, or changes owner.
//! - Within simulation, `store.get_mut(id)` borrows one actor for an update.
//! - Cross-entity reads during mutation: read target first (copy the facts
//!   needed), then get_mut on the other entity
//! - A walk that writes only each visited entity's own fields (and fields
//!   beside the store, such as an output channel): `for_each_entry_mut`,
//!   which needs no id list or lookup by id, and hands an entity out only
//!   through `EntryMut::into_mut` (every hand-out enters the touch logs; see
//!   `touched`)
//! - A sweep from the highest id down whose visits mutate the world:
//!   `descending_ids`, a cursor that copies no ids
//! - Only where neither fits, a copy of the ids: `keys_sorted()`, looping
//!   with `get_mut_if()` so an entity the walk leaves unchanged is not
//!   handed out
//!
//! ## Forks (VERA-only, sim-scale)
//! For object turns run side by side: [`EntityStore::share`] moves the
//! columns, uncopied, behind one `Arc`, and the shared store answers reads
//! and refuses writes. [`EntityStore::fork`] makes a store that reads them
//! and keeps its own writes: a copy of an entity on its first write and of a
//! sleep cell, which its later reads see, the focused object's without a
//! search. One turn runs on a fork; [`EntityStore::take_fork_writes`] takes
//! what it wrote and returns the fork to its base in time proportional to
//! that, and [`EntityStore::commit_writes`] applies it to the store once
//! [`EntityStore::unshare`]d. A write that changes a column's length or an
//! index (an insert or removal, an owner or air-membership change, a held
//! Anim, a walk writing every entity) first gives the fork its own copy of
//! every column, sharing its base's entities until it writes one: the fork
//! stays a correct store, and its writes cannot be merged
//! ([`EntityWrites::structural`]).
//! [`EntityStore::written_this_pass`] tells which entities were written since
//! [`EntityStore::begin_pass`].
//!
//! ## Dependency rules
//! - Part of sim/ — stores sim/game_entity; the shared AirTracker query reads
//!   existing Cell geometry and bucket kernels, never app/presentation state.
//! - sim/ NEVER depends on render/, ui/, audio/, net/.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use crate::map::entities::EntityCategory;
use crate::sim::game_entity::GameEntity;
use crate::sim::intern::InternedId;
use crate::sim::touch_log::{TouchLogs, Touched};
use crate::util::lookup_hash::LookupMap;

/// Only this module can authorize a live indexed-owner write.
/// Payload consumers can read owner identity but cannot construct this capability.
pub(crate) struct OwnerChangeAuthority(());

/// Only this module can write an entity's AircraftTracker membership, so the
/// airborne set ([`EntityStore::airborne`]) changes with it.
pub(crate) struct AirMembershipAuthority(());

/// Only this module can store an Anim in a field [`GameEntity::held_anims`]
/// names, so the anim-holder index notes the holder
/// ([`EntityStore::write_held_anim`]).
pub(super) struct AnimHolderAuthority(());

/// Only this module can read and forget an entity's name note
/// ([`GameEntity::take_names_unread`]), so no note is lost before the
/// pointer-expiry index takes it ([`EntityStore::take_unread_names`]).
pub(crate) struct UnreadNamesAuthority(());

/// What a write to a shared store panics with.
const SHARED_WRITE: &str = "write to a shared entity store: while shared it is the frozen base its forks read; write through a fork, or unshare it first";

/// Container for all game entities, keyed by stable_id.
///
/// All iteration methods return entities in ascending stable_id order, which
/// is critical for lockstep multiplayer.
///
/// A House's buildings in native order are its House+0x68 list
/// (`HouseBaseState::buildings`); its per-type counts are its
/// `HouseTracking`.
#[derive(Debug)]
pub struct EntityStore {
    /// The columns while the store owns them: an ordinary store's, or a
    /// fork's own copy after a structural write ([`Self::materialise`]).
    /// Empty while the store reads through `view`.
    cols: Columns,
    /// VERA-only (sim-scale): the shared columns a shared store
    /// ([`Self::share`]) or a fork ([`Self::fork`]) reads, with a fork's
    /// writes. `None` for an ordinary store, whose lookups take one
    /// predictable branch on it.
    view: Option<Box<View>>,
    /// A fork that met a structural write: its view, set aside while it
    /// reads its own copy of the columns, for [`Self::reset_fork`].
    set_aside: Option<Box<View>>,
    /// VERA-only: the id last given to [`Self::focus`] and its slot (`None`:
    /// not stored), so an object's turn, which looks its own id up about a
    /// hundred times, hashes it once ([`Self::slot`]). `slot_of` is the
    /// source of truth: every function that writes it re-reads the focused
    /// id's slot ([`Self::refocus`]), so a lookup answers the same with or
    /// without it. Never saved or hashed.
    focus: (u64, Option<u32>),
    /// Lookups by id so far, and those of them the id index answered (test
    /// builds only), for checks of what a turn looks up.
    #[cfg(any(test, feature = "test-support"))]
    lookups: [std::sync::atomic::AtomicU64; 2],
    /// Which entities may have changed since each reader last took its log
    /// ([`Self::take_touched`]). Every route to a `&mut GameEntity` goes
    /// through the store, so an entity missing from a log has not changed,
    /// but for the mission AI counter and a building's tint sample, which
    /// change every frame and no reader derives from
    /// ([`Self::count_mission_ai_visit`], [`Self::capture_building_tint_sample`]),
    /// and the name note the pointer-expiry index alone reads and forgets
    /// ([`Self::take_unread_names`]). A fork's start as a clone's do and note
    /// nothing; its commit notes its hand-outs in the store's.
    touched: TouchLogs<{ TouchReader::COUNT }>,
    /// Which entities may draw differently since the presentation last took
    /// the log ([`Self::take_looks`]): stored, or given a new owner, type or
    /// disguise. [`Self::insert`] and [`Self::change_owner`] note it, and the
    /// in-place type and disguise writers call [`Self::note_look`]. The
    /// simulation never reads it; never saved, compared or hashed.
    looks: TouchLogs<1>,
    /// The empty stand-in of a presentation copy of the world
    /// ([`Self::detached`]), whose objects are in the presentation frame
    /// instead. Debug builds refuse reads of it.
    detached: bool,
    /// VERA-only: the pass [`Self::begin_pass`] began last (0: none), whose
    /// number every write stamps in its slot's [`SleepCell`] for
    /// [`Self::written_this_pass`]. Never saved or hashed.
    pass: u16,
}

/// The stored entities by slot and the indexes derived from them: what
/// [`EntityStore::share`] puts behind one `Arc`. A clone shares the entities
/// (a fork's own copy, [`EntityStore::materialise`]); [`EntityStore`]'s clone
/// copies them.
#[derive(Debug, Clone, Default)]
struct Columns {
    /// The stored ids in ascending order, one per slot. A removed entity's
    /// slot keeps its id, with no entity, until [`EntityStore::compact`].
    ids: Vec<u64>,
    /// The entity of each slot, behind an `Arc`: an entity is about 2.2 KB,
    /// so insert, remove and compaction move pointers, not entities, and a
    /// fork's own copy of the columns shares them. A store's entities are its
    /// own but in that copy, so a hand-out ([`entity_mut`]) copies nothing
    /// anywhere else.
    entities: Vec<Option<Arc<GameEntity>>>,
    /// stable_id -> slot, for every slot that holds an entity. Only looked
    /// up, never iterated, so its order cannot reach a result.
    slot_of: LookupMap<u64, u32>,
    /// VERA-only: each slot's owner, beside the boxed entities for walks that
    /// ask only whose an object is ([`EntityStore::owner_of`]). The entity's
    /// own owner is the source of truth: [`EntityStore::insert`] copies it
    /// and [`EntityStore::change_owner`], its one live writer, writes both. A
    /// removed slot keeps a stale owner until compaction drops the slot.
    /// Rebuilt on load, never saved or hashed.
    owners: Vec<InternedId>,
    /// VERA-only: each slot's category, beside the boxed entities for walks
    /// that ask only what class an object is ([`EntityStore::category_of`]).
    /// A stored entity's category never changes (a replacement comes through
    /// [`EntityStore::insert`], which copies it). Kept and rebuilt as
    /// `owners` is.
    categories: Vec<EntityCategory>,
    /// VERA-only: each slot's sleep mark and turn marks ([`SleepCell`]),
    /// beside the boxed entities so the live object pass passes a sleeping
    /// object without loading it, and a quiet or steady one's turn skips
    /// steps without reading their state. Every route to a `&mut GameEntity`
    /// settles and clears them first ([`EntityStore::wake_slot`]), so a
    /// sleeping, quiet or steady entity is one nothing has written since its
    /// last turn. Kept through compaction as `owners` is; a load starts every
    /// object awake and unmarked. Never saved or hashed: the save form and
    /// the state hash read a sleeping object as settled.
    sleep: Vec<SleepCell>,
    /// VERA-only: the stored ids of each owner's objects of each category,
    /// ascending ([`EntityStore::owned_ids`]), for walks that want one
    /// house's objects of one class without loading the others. The
    /// entities' owners and categories are the source of truth:
    /// [`EntityStore::insert`] and [`EntityStore::remove`] keep it,
    /// [`EntityStore::change_owner`], the owners' one live writer, moves an
    /// id between lists, and compaction changes no id. Rebuilt on load
    /// ([`EntityStore::rebuild_class_registries`]), never saved or hashed,
    /// and only looked up, never iterated as a map.
    owned: LookupMap<(InternedId, EntityCategory), Vec<u64>>,
    /// Derived InfantryClass registry, in monotonic construction-ID order.
    /// Uninit retains an entry; remove compacts it. Class/category is immutable
    /// while stored, like indexed identity (replace through remove/insert).
    infantry_registry: Vec<u64>,
    /// FootClass array8B3DC4, including Aircraft and stored limbo objects.
    /// Derived from the same monotonic constructor IDs as Techno below;
    /// parent Foot registration precedes its constructor-manager children.
    /// UnInit retains rows; removal/replacement/restore update this cache.
    foot_registry: Vec<u64>,
    /// Derived TechnoClass registration index from monotonic constructor IDs.
    /// Production construction stores each parent before manager children;
    /// see construction33/55,world_spawn::store_with_constructor_managers,
    /// authored_health::unlimbo and reinforcement construction. Stable ID is
    /// authoritative, this cache is rebuilt on restore and never serialized.
    /// UnInit keeps a row; physical remove compacts the registry independently
    /// of the dense storage's empty slots and compaction schedule.
    /// RMG PreconsumedGenerated registration parity is not established here.
    techno_registry: Vec<u64>,
    /// BuildingClass::Array A8EB40, derived from immutable class and the
    /// same constructor IDs. Insert/remove/replace/restore maintain it like
    /// the other class registries; UnInit alone retains its row.
    building_registry: Vec<u64>,
    /// VERA-only: `(bucket, enter order, stable_id)` of every stored entity
    /// the AircraftTracker holds (an air bucket): each bucket's entries in
    /// the order [`EntityStore::airborne_in_query_order`] copies them. The
    /// entity's tracker membership is the source of truth:
    /// [`EntityStore::insert`] and [`EntityStore::remove`] keep this set, and
    /// [`EntityStore::set_air_membership`], the membership's one live writer,
    /// writes both. Rebuilt on load, never saved or hashed.
    airborne: BTreeSet<(u16, u64, u64)>,
    /// VERA-only: `(Anim, holder)` for each Anim a stored entity may hold in
    /// a field [`GameEntity::held_anims`] names, so an Anim's expiry
    /// (`Simulation::notify_anim_pointer_expired`, through
    /// [`EntityStore::take_anim_holders`]) visits those entities without
    /// refreshing the pointer-expiry index. A superset;
    /// the entities' fields are the truth. [`EntityStore::insert`] and
    /// [`EntityStore::write_held_anim`], the only route to storing an Anim in
    /// one of those fields, add pairs; the expiry takes an Anim's pairs, its
    /// callbacks clearing every field that names it; a removal drops the
    /// removed entity's. A field cleared otherwise leaves its pair until
    /// then. Rebuilt on load, never saved or hashed.
    anim_holders: BTreeSet<(u64, u64)>,
}

/// One slot's sleep mark (sim-scale; results unchanged). An idle object's
/// turn changes nothing but its per-visit counters until one of its timers
/// comes due, so the live object pass may count the visit as owed instead of
/// running it (`Simulation::advance_live_object_pass`).
#[derive(Debug, Clone, Copy, Default)]
struct SleepCell {
    /// The first frame whose turn must run; 0 while awake.
    wake_at: u32,
    /// Visits the pass counted instead of running.
    owed: u32,
    /// What the sleeper's owner looked like when it fell asleep, for the
    /// pass to compare (`world::idle_sleep`).
    note: u8,
    /// The frame after the last turn that left the object fit to sleep,
    /// while awake; 0 for none ([`EntityStore::fit_to_sleep`]).
    fit_until: u32,
    /// What the object's last turn left it as ([`QUIET`], [`STEADY`]), with
    /// nothing handed it for writing since. Every mutable hand-out clears
    /// them, asleep or awake.
    marks: u8,
    /// The last pass ([`EntityStore::begin_pass`]) that wrote the slot's
    /// entity or this cell. Not part of the mark: waking keeps it.
    written: u16,
}

// The stamp sits in what was padding: a cell stays a quarter of a line.
const _: () = assert!(std::mem::size_of::<SleepCell>() == 16);

/// A [`SleepCell`] mark: the object is quiet (`world::quiet_units`), its
/// last turn having left every state its quiet turn skips off.
const QUIET: u8 = 1;
/// A [`SleepCell`] mark: the object is steady (`world::steady_ai`), its last
/// turn having left every step of its AI visit off but its counter, its
/// timers and its stage clock.
const STEADY: u8 = 2;
/// A [`SleepCell`] mark: the ground locomotor's next Process only samples slope.
const LOCOMOTOR_RESTS: u8 = 4;

/// What a shared store or a fork reads through.
#[derive(Debug)]
struct View {
    /// The shared columns, frozen while any fork of them lives.
    base: Arc<Columns>,
    /// A fork's view, which keeps its writes in `overlay`; the shared store
    /// itself refuses writes.
    fork: bool,
    /// The fork's writes since its last take or reset.
    overlay: Overlay,
}

/// A fork's writes: the entities and sleep cells it wrote, by id, and the
/// looks it noted.
#[derive(Debug, Clone, Default)]
struct Overlay {
    /// The focused id's writes, so the turn's own object costs no search.
    focus: Written,
    /// Every other id written, in the order first written: about one turn
    /// in a hundred writes one, so a linear search serves.
    others: Vec<Written>,
    /// The ids [`EntityStore::note_look`] noted, in order.
    looks: Vec<u64>,
}

/// What a fork wrote of one id.
#[derive(Debug, Clone, Default)]
struct Written {
    id: u64,
    /// The id's slot in the base, once written.
    slot: u32,
    /// The fork's copy of the entity, made from the base's on the first
    /// write.
    entity: Option<Arc<GameEntity>>,
    /// The fork's copy of the sleep cell, made on its first write.
    cell: Option<SleepCell>,
    /// A hand-out took the entity, which the touch logs note.
    noted: bool,
}

/// One object's turn's writes in a fork ([`EntityStore::take_fork_writes`]),
/// for [`EntityStore::commit_writes`] on the store the fork's base was shared
/// from: each entity the turn wrote (its id, slot and new state), each sleep
/// cell, and the looks it noted.
#[derive(Debug)]
pub(crate) struct EntityWrites {
    overlay: Overlay,
    /// The turn met a structural write; nothing else is kept.
    structural: bool,
}

#[cfg_attr(
    not(any(test, feature = "test-support")),
    expect(dead_code, reason = "no pass runs object turns on forks yet")
)]
impl EntityWrites {
    /// The turn changed a column's length or an index (see the module
    /// documentation): its writes cannot be merged, and it runs again on the
    /// real store.
    pub(crate) fn structural(&self) -> bool {
        self.structural
    }

    /// The ids whose entity or sleep cell the turn wrote, each once.
    pub(crate) fn ids(&self) -> impl Iterator<Item = u64> + '_ {
        self.overlay.records().map(|written| written.id)
    }
}

/// How a fork's write takes an entity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ForkWrite {
    /// A mutable hand-out: woken first, and noted in the touch logs at the
    /// commit.
    HandOut,
    /// A write the touch logs' contract excepts, woken first
    /// ([`EntityStore::unlogged_mut`]).
    Unlogged,
    /// The mission AI counter's step, neither woken nor noted
    /// ([`EntityStore::count_mission_ai_visit`]).
    Counter,
}

/// A product derived from the entities that re-derives from the touch log.
/// Each reads its own log, so one reader's take leaves the others' intact.
#[derive(Debug, Clone, Copy)]
pub(crate) enum TouchReader {
    /// The movement pass's kept blocker plane.
    BlockerPlane = 0,
    /// The pointer-expiry broadcast's listener index, which re-reads only
    /// the noted entities among those logged ([`EntityStore::take_unread_names`]).
    ExpiryListeners = 1,
}

impl TouchReader {
    const COUNT: usize = 2;
}

impl Clone for EntityStore {
    /// A clone is an ordinary store holding what this one reads, a fork's
    /// writes included. It starts with an everything-touched log, as
    /// whatever was derived from the original says nothing certain about
    /// the copy's future, and with no slot written this pass.
    fn clone(&self) -> Self {
        let mut cols = match self.view.as_deref() {
            Some(view) => {
                let mut cols = Columns::clone(&view.base);
                view.overlay.clone().apply(&mut cols);
                cols
            }
            None => self.cols.clone(),
        };
        // The columns' clone shares the entities; the copy holds its own.
        for entity in cols.entities.iter_mut().flatten() {
            *entity = Arc::new(GameEntity::clone(entity));
        }
        for cell in &mut cols.sleep {
            cell.written = 0;
        }
        Self {
            cols,
            view: None,
            set_aside: None,
            focus: self.focus,
            #[cfg(any(test, feature = "test-support"))]
            lookups: Default::default(),
            touched: TouchLogs::everything(),
            looks: TouchLogs::everything(),
            detached: self.detached,
            pass: 0,
        }
    }
}

impl EntityStore {
    /// Take `reader`'s touch log, leaving it empty.
    pub(crate) fn take_touched(&mut self, reader: TouchReader) -> Touched {
        self.touched.take(reader as usize)
    }

    /// Whether `reader`'s touch log keeps only buildings' hand-outs from now
    /// on ([`TouchLogs::set_narrow`]): the blocker plane's, while only
    /// buildings are its sources. A store hands out most objects through
    /// [`Self::get_mut`], [`Self::get_mut_if`] and [`EntryMut::into_mut`],
    /// which say which are buildings, as a fork's commit does
    /// ([`Self::commit_writes`]); any other hand-out is noted in every log.
    pub(crate) fn narrow_touched(&mut self, reader: TouchReader, narrow: bool) {
        self.touched.set_narrow(reader as usize, narrow);
    }

    /// Note the hand-out of `stable_id`, stored in `slot`, in the touch logs,
    /// where every object but a building is broad ([`Self::narrow_touched`]).
    #[inline]
    fn note_hand_out(&mut self, stable_id: u64, slot: usize) {
        let broad = self.cols.categories[slot] != EntityCategory::Structure;
        self.touched
            .note_kind(stable_id, broad, self.cols.slot_of.len());
    }

    /// The entities the pointer-expiry index must re-read (sim-scale;
    /// results unchanged): of those handed out since it last asked, the
    /// removed ones and the stored ones noted since
    /// ([`GameEntity::note_names`]), or every one when the log cannot say
    /// which. A field that names an object takes an id only through a writer
    /// that takes that note, so an entity left out names at most what the
    /// index last read of it. Not a hand-out and no lookup for a caller:
    /// forgetting the transient note neither wakes an object nor notes it in
    /// the logs or the lookup counts.
    ///
    /// The note is the entity's own: a fork's write notes the fork's copy
    /// (a copy starts noted), which [`Self::commit_writes`] moves in and logs
    /// as a hand-out, so this store's next take reads it once. A shared
    /// store or a fork forgets no note, its entities being its base's: it
    /// gives its log as it is.
    pub(crate) fn take_unread_names(&mut self) -> Touched {
        let logged = self.touched.take(TouchReader::ExpiryListeners as usize);
        if self.view.is_some() || self.set_aside.is_some() {
            return logged;
        }
        let cols = &mut self.cols;
        match logged {
            Touched::All => {
                for entity in cols.entities.iter_mut().filter_map(entity_mut) {
                    entity.take_names_unread(UnreadNamesAuthority(()));
                }
                Touched::All
            }
            Touched::Ids(mut ids) => {
                ids.retain(|id| match cols.slot_of.get(id) {
                    Some(&slot) => entity_mut(&mut cols.entities[slot as usize])
                        .is_some_and(|entity| entity.take_names_unread(UnreadNamesAuthority(()))),
                    None => true,
                });
                Touched::Ids(ids)
            }
        }
    }

    /// Take the look log, leaving it empty.
    pub(crate) fn take_looks(&mut self) -> Touched {
        self.looks.take(0)
    }

    /// Note that an in-place write changed how `stable_id` draws: its type
    /// or a disguise it puts on. A fork keeps the note for its commit.
    pub(crate) fn note_look(&mut self, stable_id: u64) {
        if self.view.is_some() {
            fork_view(&mut self.view).overlay.looks.push(stable_id);
            return;
        }
        self.looks.note(stable_id, self.cols.slot_of.len());
    }

    /// Mark `stable_id` asleep until frame `wake_at` (above 0): the pass may
    /// count its visits before that frame as owed. Any mutable hand-out wakes
    /// it first.
    pub(crate) fn sleep_until(&mut self, stable_id: u64, wake_at: u32, note: u8) {
        debug_assert!(wake_at != 0, "wake frame 0 means awake");
        if let Some(cell) = self.cell_mut(stable_id) {
            cell.wake_at = wake_at;
            cell.note = note;
        }
    }

    /// `stable_id`'s turn at `frame` left it fit to sleep. Answers whether
    /// its turn the frame before did too: only then did this whole turn run
    /// on a resting object, so its samplers hold what a next turn would
    /// sample. Waking forgets it.
    pub(crate) fn fit_to_sleep(&mut self, stable_id: u64, frame: u32) -> bool {
        let Some(cell) = self.cell_mut(stable_id) else {
            return false;
        };
        let rested = cell.fit_until == frame;
        cell.fit_until = frame.wrapping_add(1);
        rested
    }

    /// `stable_id`'s turn at `frame` left something in progress.
    pub(crate) fn unfit_to_sleep(&mut self, stable_id: u64) {
        if let Some(cell) = self.cell_mut(stable_id) {
            cell.fit_until = 0;
        }
    }

    /// The frame `stable_id` sleeps until and its note, if it is asleep.
    #[inline]
    pub(crate) fn sleeps_until(&self, stable_id: u64) -> Option<(u32, u8)> {
        let cell = self.cell(stable_id)?;
        (cell.wake_at != 0).then_some((cell.wake_at, cell.note))
    }

    /// Count the visit the pass makes to sleeping `stable_id` instead of
    /// running its turn.
    pub(crate) fn owe_visit(&mut self, stable_id: u64) {
        if let Some(cell) = self.cell_mut(stable_id) {
            cell.owed = cell.owed.wrapping_add(1);
        }
    }

    /// Drop the visits `stable_id` is owed without settling them: the sleep
    /// check's, which counts one only to hash the skipped turn
    /// (`world::idle_sleep`).
    pub(crate) fn forget_owed(&mut self, stable_id: u64) {
        if let Some(cell) = self.cell_mut(stable_id) {
            cell.owed = 0;
        }
    }

    /// The visits sleeping `stable_id` is owed, which its mission counter
    /// does not hold yet ([`GameEntity::settle_sleep`]).
    pub(crate) fn owed_visits(&self, stable_id: u64) -> u32 {
        self.cell(stable_id).map_or(0, |cell| cell.owed)
    }

    /// The visits each stored entity is owed, in [`Self::values`] order.
    pub(crate) fn owed_in_value_order(&self) -> Vec<u32> {
        let cols = self.columns();
        let overlay = self.written_overlay();
        cols.entities
            .iter()
            .zip(&cols.sleep)
            .enumerate()
            .filter(|(_, (entity, _))| entity.is_some())
            .map(|(slot, (_, &cell))| {
                let cell = overlay.map_or(cell, |overlay| overlay.cell_of(cols.ids[slot], cell));
                cell.owed
            })
            .collect()
    }

    /// Wake `stable_id`, settling its owed visits.
    pub(crate) fn wake(&mut self, stable_id: u64) {
        let Some(slot) = self.slot(stable_id) else {
            return;
        };
        if self.view.is_some() {
            let focused = stable_id == self.focus.0;
            fork_view(&mut self.view).wake(stable_id, focused, slot);
        } else {
            self.wake_slot(slot as usize);
        }
    }

    /// Wake every object: for a change that sleeping objects read without
    /// being written (a house's power, a cell's hidden state).
    pub(crate) fn wake_all(&mut self) {
        self.own_columns();
        for slot in 0..self.cols.sleep.len() {
            self.wake_slot(slot);
        }
    }

    /// Whether `stable_id` is quiet: its last turn left it so, and nothing
    /// has been handed it for writing since (`world::quiet_units`).
    #[inline]
    pub(crate) fn is_quiet(&self, stable_id: u64) -> bool {
        self.cell(stable_id)
            .is_some_and(|cell| cell.marks & QUIET != 0)
    }

    /// Ask the processor to start loading `stable_id`'s record, if it is
    /// stored and awake (a sleeper's visit reads its sleep cell alone): for
    /// the live object pass, a few slots before the object's turn
    /// (`util::prefetch`). Sim-scale, results unchanged: it reads no entity,
    /// counts as no lookup and leaves the focus alone.
    #[inline]
    pub(crate) fn warm(&self, stable_id: u64) {
        let cols = self.columns();
        let Some(&slot) = cols.slot_of.get(&stable_id) else {
            return;
        };
        if cols.sleep[slot as usize].wake_at != 0 {
            return;
        }
        if let Some(entity) = cols.entities[slot as usize].as_deref() {
            crate::util::prefetch::prefetch(entity);
        }
    }

    /// `stable_id`'s entity and whether it is quiet, in one lookup.
    pub(crate) fn get_and_quiet(&self, stable_id: u64) -> Option<(&GameEntity, bool)> {
        if let Some(view) = self.view.as_deref() {
            let slot = self.slot_in(&view.base.slot_of, stable_id)?;
            let focused = stable_id == self.focus.0;
            let quiet = view.cell(stable_id, focused, slot).marks & QUIET != 0;
            return Some((view.entity(stable_id, focused, slot)?, quiet));
        }
        let slot = self.slot_in(&self.cols.slot_of, stable_id)? as usize;
        Some((
            self.cols.entities[slot].as_deref()?,
            self.cols.sleep[slot].marks & QUIET != 0,
        ))
    }

    /// Mark `stable_id` quiet until its next mutable hand-out.
    pub(crate) fn set_quiet(&mut self, stable_id: u64) {
        if let Some(cell) = self.cell_mut(stable_id) {
            cell.marks |= QUIET;
        }
    }

    /// Whether `stable_id`'s ground Process may take the resting fast path.
    #[inline]
    pub(crate) fn is_locomotor_resting(&self, stable_id: u64) -> bool {
        self.cell(stable_id)
            .is_some_and(|cell| cell.marks & LOCOMOTOR_RESTS != 0)
    }

    /// Mark `stable_id`'s ground Process resting until its next mutable hand-out.
    pub(crate) fn set_locomotor_resting(&mut self, stable_id: u64) {
        if let Some(cell) = self.cell_mut(stable_id) {
            cell.marks |= LOCOMOTOR_RESTS;
        }
    }

    /// Whether `stable_id` is steady: its last turn left it so, and nothing
    /// has been handed it for writing since (`world::steady_ai`).
    #[inline]
    pub(crate) fn is_steady(&self, stable_id: u64) -> bool {
        self.cell(stable_id)
            .is_some_and(|cell| cell.marks & STEADY != 0)
    }

    /// Mark `stable_id` steady until its next mutable hand-out.
    pub(crate) fn set_steady(&mut self, stable_id: u64) {
        if let Some(cell) = self.cell_mut(stable_id) {
            cell.marks |= STEADY;
        }
    }

    /// How many objects are asleep.
    pub(crate) fn sleeping_count(&self) -> usize {
        let cols = self.columns();
        let overlay = self.written_overlay();
        cols.sleep
            .iter()
            .enumerate()
            .filter(|&(slot, &cell)| {
                let cell = overlay.map_or(cell, |overlay| overlay.cell_of(cols.ids[slot], cell));
                cell.wake_at != 0
            })
            .count()
    }

    /// `stable_id`'s sleep cell as the store reads it.
    #[inline]
    fn cell(&self, stable_id: u64) -> Option<SleepCell> {
        if let Some(view) = self.view.as_deref() {
            let slot = self.slot_in(&view.base.slot_of, stable_id)?;
            return Some(view.cell(stable_id, stable_id == self.focus.0, slot));
        }
        Some(self.cols.sleep[self.slot_in(&self.cols.slot_of, stable_id)? as usize])
    }

    /// `stable_id`'s sleep cell for writing, stamped with the pass; a fork's
    /// own copy of it.
    #[inline]
    fn cell_mut(&mut self, stable_id: u64) -> Option<&mut SleepCell> {
        if self.view.is_some() {
            let slot = self.slot(stable_id)?;
            let focused = stable_id == self.focus.0;
            return Some(fork_view(&mut self.view).cell_mut(stable_id, focused, slot));
        }
        let slot = self.slot_in(&self.cols.slot_of, stable_id)? as usize;
        let cell = &mut self.cols.sleep[slot];
        cell.written = self.pass;
        Some(cell)
    }

    /// Clear `slot`'s sleep mark and turn marks and give its entity the
    /// visits it is owed, before anything writes it; stamp it with the pass.
    #[inline]
    fn wake_slot(&mut self, slot: usize) {
        let pass = self.pass;
        let cell = &mut self.cols.sleep[slot];
        cell.written = pass;
        if cell.wake_at == 0 {
            cell.marks = 0;
            return;
        }
        let owed = cell.owed;
        *cell = SleepCell {
            written: pass,
            ..SleepCell::default()
        };
        if owed == 0 {
            return;
        }
        // Waking is not a hand-out: no note in the touch logs.
        if let Some(entity) = entity_mut(&mut self.cols.entities[slot]) {
            entity.settle_sleep(owed);
        }
    }

    /// A new slot's cell: awake, unmarked, written this pass.
    fn fresh_cell(&self) -> SleepCell {
        SleepCell {
            written: self.pass,
            ..SleepCell::default()
        }
    }

    /// How many mutable hand-outs the store has made (test builds only).
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn hand_outs(&self) -> u64 {
        self.touched.hand_outs()
    }

    /// How many lookups by id the store has answered, and how many of them
    /// through the id index rather than the focused slot (test builds only).
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn lookups(&self) -> (u64, u64) {
        let count = |counter: &std::sync::atomic::AtomicU64| {
            counter.load(std::sync::atomic::Ordering::Relaxed)
        };
        (count(&self.lookups[0]), count(&self.lookups[1]))
    }

    /// Create an empty store.
    pub fn new() -> Self {
        Self {
            cols: Columns::default(),
            view: None,
            set_aside: None,
            // An empty store holds no id.
            focus: (0, None),
            #[cfg(any(test, feature = "test-support"))]
            lookups: Default::default(),
            touched: TouchLogs::everything(),
            looks: TouchLogs::everything(),
            detached: false,
            pass: 0,
        }
    }

    /// The empty store a presentation copy of the world holds in place of
    /// the objects (`Simulation::presentation_copy`): drawing reads them
    /// from its presentation frame. Debug builds panic on a read of it, so a
    /// reader still reaching for the world's objects shows up in any draw.
    pub(crate) fn detached() -> Self {
        Self {
            detached: true,
            ..Self::new()
        }
    }

    /// Debug builds: refuse a read of a [`Self::detached`] store.
    #[inline]
    fn check_attached(&self) {
        debug_assert!(
            !self.detached,
            "read of the objects of a presentation copy of the world: read the presentation frame's records instead"
        );
    }

    /// The columns the store reads: its own, or the shared ones it reads
    /// through.
    #[inline]
    fn columns(&self) -> &Columns {
        match self.view.as_deref() {
            Some(view) => &view.base,
            None => &self.cols,
        }
    }

    /// A fork's writes, if it has written an entity or a sleep cell.
    fn written_overlay(&self) -> Option<&Overlay> {
        self.view
            .as_deref()
            .map(|view| &view.overlay)
            .filter(|overlay| overlay.is_written())
    }

    /// The entity in `slot`, which `stable_id`'s is, as the store reads it.
    #[inline]
    fn entity_at(&self, stable_id: u64, slot: u32) -> Option<&GameEntity> {
        match self.view.as_deref() {
            Some(view) => view.entity(stable_id, stable_id == self.focus.0, slot),
            None => self.cols.entities[slot as usize].as_deref(),
        }
    }

    /// A fork's write of `stable_id`'s entity, taken as `write` says: the
    /// fork's copy, made from its base's on the first write. A hand-out is
    /// kept for the commit to note; the fork's own logs note nothing.
    fn fork_entity_mut(&mut self, stable_id: u64, write: ForkWrite) -> Option<&mut GameEntity> {
        let slot = self.slot(stable_id)?;
        let focused = stable_id == self.focus.0;
        let view = fork_view(&mut self.view);
        if write != ForkWrite::Counter {
            view.wake(stable_id, focused, slot);
        }
        let written = view.copied(stable_id, focused, slot);
        written.noted |= write == ForkWrite::HandOut;
        entity_mut(&mut written.entity)
    }

    /// Before a write that changes a column's length or an index: a fork
    /// takes its own copy of the columns ([`Self::materialise`]), and a
    /// shared store refuses. No-op for an ordinary store.
    #[inline]
    fn own_columns(&mut self) {
        if self.view.is_some() {
            self.materialise();
        }
    }

    /// Give a fork its own copy of its base, its writes applied, and set its
    /// view aside: from here to [`Self::reset_fork`] it is an ordinary store
    /// whose writes cannot be merged ([`EntityWrites::structural`]). The copy
    /// shares the base's entities, a reference count each, until a write
    /// copies the one it writes ([`entity_mut`]).
    #[cold]
    #[inline(never)]
    fn materialise(&mut self) {
        let mut view = self.view.take().expect("a store reading shared columns");
        if !view.fork {
            self.view = Some(view);
            panic!("{SHARED_WRITE}");
        }
        let mut cols = Columns::clone(&view.base);
        std::mem::take(&mut view.overlay).apply(&mut cols);
        self.cols = cols;
        self.set_aside = Some(view);
    }

    /// Insert an entity, replacing one of the same id (rare — stable_ids are
    /// monotonic). Returns its stable_id.
    pub fn insert(&mut self, mut entity: GameEntity) -> u64 {
        self.own_columns();
        let id = entity.stable_id();
        // The pointer-expiry index reads a stored entity whole, whatever
        // was written to it outside the store.
        let _ = entity.note_names();
        self.touched.note(id, self.len());
        self.looks.note(id, self.len());
        let category = entity.category();
        let infantry = category == EntityCategory::Infantry;
        let owner = entity.owner();
        let foot = category.is_foot();
        // A replaced entity leaves its owner's list and the airborne set;
        // this one joins its owner's list, and the airborne set if the
        // tracker holds it.
        if let Some((key, entry)) = self.get(id).map(|replaced| {
            (
                (replaced.owner(), replaced.category()),
                airborne_entry(replaced),
            )
        }) {
            self.unlist_owned(id, key);
            if let Some(entry) = entry {
                self.cols.airborne.remove(&entry);
            }
        }
        self.list_owned(id, (owner, category));
        if let Some(entry) = airborne_entry(&entity) {
            self.cols.airborne.insert(entry);
        }
        self.cols
            .anim_holders
            .extend(entity.held_anims().map(|anim| (anim, id)));
        let entity = Some(Arc::new(entity));
        let cell = self.fresh_cell();
        let cols = &mut self.cols;
        if let Some(&slot) = cols.slot_of.get(&id) {
            cols.entities[slot as usize] = entity;
            cols.owners[slot as usize] = owner;
            cols.categories[slot as usize] = category;
            cols.sleep[slot as usize] = cell;
        } else if cols.ids.last().is_none_or(|&last| last < id) {
            cols.slot_of.insert(id, cols.ids.len() as u32);
            cols.ids.push(id);
            cols.entities.push(entity);
            cols.owners.push(owner);
            cols.categories.push(category);
            cols.sleep.push(cell);
        } else {
            // An id below the newest: a fixture's, or a removed id stored again.
            let slot = cols.ids.partition_point(|&stored| stored < id);
            if cols.ids[slot] == id {
                cols.entities[slot] = entity;
                cols.owners[slot] = owner;
                cols.categories[slot] = category;
                cols.sleep[slot] = cell;
                cols.slot_of.insert(id, slot as u32);
            } else {
                cols.ids.insert(slot, id);
                cols.entities.insert(slot, entity);
                cols.owners.insert(slot, owner);
                cols.categories.insert(slot, category);
                cols.sleep.insert(slot, cell);
                self.reindex_from(slot);
            }
        }
        self.refocus();
        let cols = &mut self.cols;
        if let Err(index) = cols.techno_registry.binary_search(&id) {
            cols.techno_registry.insert(index, id);
        }
        if let Ok(index) = cols.building_registry.binary_search(&id) {
            cols.building_registry.remove(index);
        }
        if category == EntityCategory::Structure {
            let index = cols
                .building_registry
                .partition_point(|&existing| existing < id);
            cols.building_registry.insert(index, id);
        }
        self.remove_infantry_index(id);
        let cols = &mut self.cols;
        if infantry {
            let index = cols
                .infantry_registry
                .partition_point(|&existing| existing < id);
            cols.infantry_registry.insert(index, id);
        }
        if let Ok(index) = cols.foot_registry.binary_search(&id) {
            cols.foot_registry.remove(index);
        }
        if foot {
            let index = cols
                .foot_registry
                .partition_point(|&existing| existing < id);
            cols.foot_registry.insert(index, id);
        }
        id
    }

    /// Remove an entity by stable_id. Returns the removed entity if it existed.
    pub fn remove(&mut self, stable_id: u64) -> Option<GameEntity> {
        if self.view.is_some() {
            if !self.contains(stable_id) {
                return None;
            }
            self.materialise();
        }
        let slot = *self.cols.slot_of.get(&stable_id)? as usize;
        self.wake_slot(slot);
        self.cols.slot_of.remove(&stable_id);
        self.refocus();
        let removed = self.cols.entities[slot].take().map(Arc::unwrap_or_clone);
        if let Some(entity) = &removed {
            self.unlist_owned(stable_id, (entity.owner(), entity.category()));
            if let Some(entry) = airborne_entry(entity) {
                self.cols.airborne.remove(&entry);
            }
            for anim in entity.held_anims() {
                self.cols.anim_holders.remove(&(anim, stable_id));
            }
        }
        self.touched.note(stable_id, self.len());
        self.remove_infantry_index(stable_id);
        let cols = &mut self.cols;
        if let Ok(index) = cols.foot_registry.binary_search(&stable_id) {
            cols.foot_registry.remove(index);
        }
        if let Ok(index) = cols.techno_registry.binary_search(&stable_id) {
            cols.techno_registry.remove(index);
        }
        if let Ok(index) = cols.building_registry.binary_search(&stable_id) {
            cols.building_registry.remove(index);
        }
        // Compact once a quarter of the slots are empty, so a walk skips few
        // and compaction's cost spreads over many removals.
        let empty = cols.ids.len() - cols.slot_of.len();
        if empty >= 64 && empty * 4 >= cols.ids.len() {
            self.compact();
        }
        removed
    }

    /// Drop the empty slots, keeping the order, and re-index. A slot's cell,
    /// its stamp with it, moves with it.
    fn compact(&mut self) {
        let cols = &mut self.cols;
        let mut kept = 0;
        for slot in 0..cols.ids.len() {
            if cols.entities[slot].is_some() {
                cols.ids.swap(kept, slot);
                cols.entities.swap(kept, slot);
                cols.owners.swap(kept, slot);
                cols.categories.swap(kept, slot);
                cols.sleep.swap(kept, slot);
                kept += 1;
            }
        }
        cols.ids.truncate(kept);
        cols.entities.truncate(kept);
        cols.owners.truncate(kept);
        cols.categories.truncate(kept);
        cols.sleep.truncate(kept);
        self.reindex_from(0);
    }

    /// Re-point the index at every stored slot from `first` on.
    fn reindex_from(&mut self, first: usize) {
        let cols = &mut self.cols;
        for slot in first..cols.ids.len() {
            if cols.entities[slot].is_some() {
                cols.slot_of.insert(cols.ids[slot], slot as u32);
            }
        }
        self.refocus();
    }

    /// Remember `stable_id`'s slot, so lookups of it skip the id index until
    /// another id is focused. For the object whose turn it is; an id the
    /// store does not hold is remembered as absent. No lookup's answer
    /// changes. A fork's writes of the id move to its focused entry.
    pub(crate) fn focus(&mut self, stable_id: u64) {
        // Every writer of `slot_of` re-reads the focused slot, so focusing
        // the same id again has nothing to look up.
        if self.focus.0 == stable_id {
            return;
        }
        self.focus.0 = stable_id;
        self.refocus();
        if let Some(view) = self.view.as_deref_mut()
            && view.fork
        {
            view.overlay.refocus(stable_id);
        }
    }

    /// Re-read the focused id's slot after a write to `slot_of`.
    fn refocus(&mut self) {
        self.focus.1 = self.columns().slot_of.get(&self.focus.0).copied();
    }

    /// `stable_id`'s slot, if it is stored: the focused id's without the id
    /// index.
    #[inline]
    fn slot(&self, stable_id: u64) -> Option<u32> {
        self.slot_in(&self.columns().slot_of, stable_id)
    }

    /// `stable_id`'s slot by `slot_of`, the columns' the store reads: the
    /// focused id's without it. Debug builds check the focused slot against
    /// the index (the live-read check).
    #[inline]
    fn slot_in(&self, slot_of: &LookupMap<u64, u32>, stable_id: u64) -> Option<u32> {
        self.check_attached();
        #[cfg(any(test, feature = "test-support"))]
        self.lookups[0].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if self.focus.0 != stable_id {
            #[cfg(any(test, feature = "test-support"))]
            self.lookups[1].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            return slot_of.get(&stable_id).copied();
        }
        debug_assert!(
            !crate::sim::touch_log::live_read_check_enabled()
                || self.focus.1 == slot_of.get(&stable_id).copied(),
            "the focused slot disagrees with the id index for entity {stable_id}"
        );
        self.focus.1
    }

    /// The stored entities in ascending stable_id order, with their ids.
    fn stored(&self) -> Stored<'_> {
        self.check_attached();
        let cols = self.columns();
        Stored {
            ids: cols.ids.iter(),
            entities: cols.entities.iter(),
            remaining: cols.slot_of.len(),
            overlay: self.written_overlay(),
        }
    }

    /// Clear all RadioClass-style live contacts involving `stable_id`.
    ///
    /// Idempotent. Safe if `stable_id` is absent.
    pub fn clear_radio_contacts_for(&mut self, stable_id: u64) {
        self.own_columns();
        // The walk below writes every entity.
        self.wake_all();
        let stored = self.len();
        self.touched.note(stable_id, stored);
        for entity in self.cols.entities.iter_mut().filter_map(entity_mut) {
            if entity.has_live_contact_with(stable_id)
                || entity.seldom.tether.partner() == Some(stable_id)
            {
                self.touched.note(entity.stable_id(), stored);
            }
            entity.clear_live_contact_with(stable_id);
            // Drop a dangling tether to the departing entity (the BREAK
            // cascade a limbo'd dock partner would otherwise miss).
            entity.seldom.tether.forget(stable_id);
            if entity.stable_id() == stable_id {
                entity.seldom.radio_contacts.clear_all();
                entity.seldom.tether.release();
            }
        }
    }

    /// Look up an entity by stable_id (immutable).
    pub fn get(&self, stable_id: u64) -> Option<&GameEntity> {
        if let Some(view) = self.view.as_deref() {
            let slot = self.slot_in(&view.base.slot_of, stable_id)?;
            return view.entity(stable_id, stable_id == self.focus.0, slot);
        }
        let slot = self.slot_in(&self.cols.slot_of, stable_id)?;
        self.cols.entities[slot as usize].as_deref()
    }

    /// `stable_id`'s owner, read from the owner column without touching the
    /// entity. Debug builds check it against the entity (the live-read check).
    pub(crate) fn owner_of(&self, stable_id: u64) -> Option<InternedId> {
        let slot = self.slot(stable_id)?;
        let owner = self.columns().owners[slot as usize];
        debug_assert!(
            !crate::sim::touch_log::live_read_check_enabled()
                || self.entity_at(stable_id, slot).map(GameEntity::owner) == Some(owner),
            "the owner column disagrees with entity {stable_id}"
        );
        Some(owner)
    }

    /// `stable_id`'s category, read from the category column without touching
    /// the entity. Debug builds check it against the entity (the live-read
    /// check).
    pub(crate) fn category_of(&self, stable_id: u64) -> Option<EntityCategory> {
        let slot = self.slot(stable_id)?;
        let category = self.columns().categories[slot as usize];
        debug_assert!(
            !crate::sim::touch_log::live_read_check_enabled()
                || self.entity_at(stable_id, slot).map(GameEntity::category) == Some(category),
            "the category column disagrees with entity {stable_id}"
        );
        Some(category)
    }

    /// `stable_id`'s entity and the slot it is stored in. Slots stay put
    /// while the store is borrowed, so tables built under one borrow may
    /// index by them; every slot is below [`Self::slot_count`].
    pub(crate) fn get_in_slot(&self, stable_id: u64) -> Option<(usize, &GameEntity)> {
        let slot = self.slot(stable_id)?;
        Some((slot as usize, self.entity_at(stable_id, slot)?))
    }

    /// One more than the highest slot.
    pub(crate) fn slot_count(&self) -> usize {
        self.columns().entities.len()
    }

    /// Mutate ordinary entity payload. Indexed identity is read through accessors;
    /// live ownership changes use the world lifecycle and `change_owner` below.
    /// Replacing a stored entity wholesale is unsupported: remove/insert through
    /// the owning lifecycle instead so its indexes and registrations are updated.
    pub(super) fn get_mut(&mut self, stable_id: u64) -> Option<&mut GameEntity> {
        if self.view.is_some() {
            return self.fork_entity_mut(stable_id, ForkWrite::HandOut);
        }
        let slot = self.slot_in(&self.cols.slot_of, stable_id)?;
        self.note_hand_out(stable_id, slot as usize);
        self.wake_slot(slot as usize);
        entity_mut(&mut self.cols.entities[slot as usize])
    }

    /// Edit one actor when a fixture holds the store directly. A fixture's
    /// setters may store any id, so the entity is noted for the
    /// pointer-expiry index too ([`GameEntity::note_names`]).
    #[cfg(any(test, feature = "test-support"))]
    pub fn get_mut_for_test(&mut self, stable_id: u64) -> Option<&mut GameEntity> {
        let entity = self.get_mut(stable_id)?;
        let _ = entity.note_names();
        Some(entity)
    }

    /// `get_mut` for an entity `admit` accepts, which it reads first: one it
    /// refuses is not handed out, so it stays out of the touch logs. For walks
    /// that visit every entity and change few.
    pub(super) fn get_mut_if(
        &mut self,
        stable_id: u64,
        admit: impl FnOnce(&GameEntity) -> bool,
    ) -> Option<&mut GameEntity> {
        if self.view.is_some() {
            let slot = self.slot(stable_id)?;
            if !admit(self.entity_at(stable_id, slot)?) {
                return None;
            }
            return self.fork_entity_mut(stable_id, ForkWrite::HandOut);
        }
        let slot = self.slot_in(&self.cols.slot_of, stable_id)? as usize;
        // Read through a shared reference: only an admitted entity is
        // handed out and noted in the touch logs.
        if !admit(self.cols.entities[slot].as_deref()?) {
            return None;
        }
        self.note_hand_out(stable_id, slot);
        self.wake_slot(slot);
        entity_mut(&mut self.cols.entities[slot])
    }

    /// Visit the stored entities whose category `keep` selects, in ascending
    /// stable_id order, without an id list or a lookup by id. `visit` reads
    /// each through [`EntryMut::get`] and borrows it for writing through
    /// [`EntryMut::into_mut`], which is a hand-out as [`Self::get_mut`] makes
    /// one; an entity only read stays out of the touch logs. For walks that
    /// write only the visited entity's own fields.
    pub(super) fn for_each_entry_mut(
        &mut self,
        keep: impl Fn(EntityCategory) -> bool,
        mut visit: impl FnMut(EntryMut<'_>),
    ) {
        self.check_attached();
        for slot in 0..self.columns().entities.len() {
            let cols = self.columns();
            if cols.entities[slot].is_some() && keep(cols.categories[slot]) {
                visit(EntryMut { store: self, slot });
            }
        }
    }

    /// Step `stable_id`'s mission AI counter (`+0xC4`) for one AI visit
    /// (`0x006FA646..0x006FA64F`) without a note in the touch logs: every
    /// techno steps it every frame, and no product derived from the logs
    /// reads it. No-op if the entity is absent.
    pub(crate) fn count_mission_ai_visit(&mut self, stable_id: u64) {
        // Not a hand-out: stepping the counter neither wakes the object nor
        // notes it in the logs.
        if self.view.is_some() {
            if let Some(entity) = self.fork_entity_mut(stable_id, ForkWrite::Counter) {
                entity.mission.count_ai_visit();
            }
            return;
        }
        if let Some(slot) = self.slot_in(&self.cols.slot_of, stable_id) {
            let slot = slot as usize;
            self.cols.sleep[slot].written = self.pass;
            if let Some(entity) = entity_mut(&mut self.cols.entities[slot]) {
                entity.mission.count_ai_visit();
            }
        }
    }

    /// Keep a building's relight light at `frame` for the draw
    /// ([`crate::sim::building_art::BuildingArt::capture_tint_sample`]),
    /// unless the kept one is current
    /// ([`crate::sim::building_art::BuildingArt::tint_sample_is_current`]),
    /// without a note in the touch logs: every building relights every
    /// frame, and no product derived from the logs reads the sample. No-op
    /// for an absent entity or one that is not a building.
    pub(crate) fn capture_building_tint_sample(
        &mut self,
        stable_id: u64,
        frame: u32,
        airstrike_active: bool,
    ) {
        // Read first: a building whose sample already says what the draw
        // would read is not handed out.
        if self.get(stable_id).is_none_or(|entity| {
            entity.category() != EntityCategory::Structure
                || entity.building_art.tint_sample_is_current(
                    entity.flashing.flashed(),
                    entity.invulnerability.as_ref(),
                    &entity.airstrike_tint,
                    airstrike_active,
                )
        }) {
            return;
        }
        if let Some(entity) = self.unlogged_mut(stable_id) {
            entity.building_art.capture_tint_sample(
                frame,
                entity.flashing.flashed(),
                entity.invulnerability.as_ref(),
                &entity.airstrike_tint,
                airstrike_active,
            );
        }
    }

    /// `stable_id`'s entity without a note: only for the writes the touch
    /// log's contract excepts.
    fn unlogged_mut(&mut self, stable_id: u64) -> Option<&mut GameEntity> {
        if self.view.is_some() {
            return self.fork_entity_mut(stable_id, ForkWrite::Unlogged);
        }
        let slot = *self.cols.slot_of.get(&stable_id)?;
        self.wake_slot(slot as usize);
        entity_mut(&mut self.cols.entities[slot as usize])
    }

    /// Check if an entity exists.
    pub fn contains(&self, stable_id: u64) -> bool {
        self.slot(stable_id).is_some()
    }

    /// Number of entities in the store.
    pub fn len(&self) -> usize {
        self.check_attached();
        self.columns().slot_of.len()
    }

    /// Whether the store is empty.
    pub fn is_empty(&self) -> bool {
        self.check_attached();
        self.columns().slot_of.is_empty()
    }

    /// A copy of the stored ids in ascending order. Right only where the
    /// walk needs the ids as they stood when it began, in ascending order,
    /// while its visits change the store through the whole world: a visit
    /// that may insert or remove entities below the walk, or callers outside
    /// the simulation. A walk that writes only the visited entity uses
    /// [`Self::for_each_entry_mut`]; a sweep from the highest id down uses
    /// [`Self::descending_ids`]; both copy no ids.
    pub fn keys_sorted(&self) -> Vec<u64> {
        self.stored().map(|(id, _)| id).collect()
    }

    /// A walk of the stored ids whose category `keep` selects, from the
    /// highest down, that holds no borrow of the store between steps
    /// ([`DescendingIds::next`]): for sweeps whose visits mutate the world,
    /// without a copy of every id.
    pub(crate) fn descending_ids(keep: fn(EntityCategory) -> bool) -> DescendingIds {
        DescendingIds { below: None, keep }
    }

    /// The stored ids of the entities whose category `keep` selects, in
    /// ascending stable_id order, chosen by the category column without
    /// loading an entity.
    pub(crate) fn ids_of_categories(&self, keep: impl Fn(EntityCategory) -> bool) -> Vec<u64> {
        self.check_attached();
        let cols = self.columns();
        cols.ids
            .iter()
            .zip(&cols.categories)
            .zip(&cols.entities)
            .filter(|&((_, &category), entity)| entity.is_some() && keep(category))
            .map(|((&id, _), _)| id)
            .collect()
    }

    /// The ids of the stored entities `keep` selects, in ascending stable_id
    /// order. `keep` runs on worker threads ([`crate::sim::parallel`]), so it
    /// only reads.
    pub(crate) fn par_select(&self, keep: impl Fn(u64, &GameEntity) -> bool + Sync) -> Vec<u64> {
        let cols = self.columns();
        let overlay = self.written_overlay();
        crate::sim::parallel::select_indices(cols.ids.len(), |slot| {
            cols.entities[slot].as_deref().is_some_and(|entity| {
                let id = cols.ids[slot];
                let entity = overlay.map_or(entity, |overlay| overlay.entity_of(id, entity));
                keep(id, entity)
            })
        })
        .into_iter()
        .map(|slot| cols.ids[slot])
        .collect()
    }

    /// `work`'s answer for every stored entity, in ascending stable_id
    /// order, each written in its place in the result. `work` runs on worker
    /// threads ([`crate::sim::parallel`]), so it only reads.
    pub fn par_map<'a, R: Send>(
        &'a self,
        work: impl Fn(u64, &'a GameEntity) -> R + Sync,
    ) -> Vec<R> {
        let stored: Vec<(u64, &GameEntity)> = self.stored().collect();
        crate::sim::parallel::map(&stored, |&(id, entity)| work(id, entity))
    }

    /// `pick`'s answers for the stored entities it answers, in ascending
    /// stable_id order. `pick` runs on worker threads
    /// ([`crate::sim::parallel`]), so it only reads.
    pub fn par_filter_map<'a, R: Send>(
        &'a self,
        pick: impl Fn(u64, &'a GameEntity) -> Option<R> + Sync,
    ) -> Vec<R> {
        let cols = self.columns();
        let overlay = self.written_overlay();
        crate::sim::parallel::filter_map_indices(cols.ids.len(), |slot| {
            let entity = cols.entities[slot].as_deref()?;
            let id = cols.ids[slot];
            let entity = overlay.map_or(entity, |overlay| overlay.entity_of(id, entity));
            pick(id, entity)
        })
    }

    /// Action14's full Techno array, independent of Logic and Display order.
    /// Read length/slot afresh after callbacks; append/removal is visible.
    pub(crate) fn techno_registry_at(&self, index: usize) -> Option<u64> {
        self.columns().techno_registry.get(index).copied()
    }
    pub(crate) fn techno_registry_len(&self) -> usize {
        self.columns().techno_registry.len()
    }

    /// UnitClass::Array8B410C is the Unit subset of constructor registration
    /// order. Derive this one-shot reader view from the existing registry;
    /// there is no second class list to save or keep in sync.
    pub(crate) fn unit_registry_ids(&self) -> impl Iterator<Item = u64> + '_ {
        self.columns()
            .techno_registry
            .iter()
            .copied()
            .filter(|&id| self.category_of(id) == Some(EntityCategory::Unit))
    }

    pub(crate) fn building_registry_at(&self, index: usize) -> Option<u64> {
        self.columns().building_registry.get(index).copied()
    }
    pub(crate) fn building_registry_len(&self) -> usize {
        self.columns().building_registry.len()
    }

    pub(crate) fn foot_registry_at(&self, index: usize) -> Option<u64> {
        self.columns().foot_registry.get(index).copied()
    }

    pub(crate) fn foot_registry_len(&self) -> usize {
        self.columns().foot_registry.len()
    }

    /// Native InfantryClass array index. Read afresh after a callback: removing
    /// a row shifts successors left, while a newly constructed row is visible.
    /// This is distinct from the live Logic vector and includes dead/limbo rows.
    pub(crate) fn infantry_registry_at(&self, index: usize) -> Option<u64> {
        self.columns().infantry_registry.get(index).copied()
    }

    /// The native InfantryClass array's length.
    pub(crate) fn infantry_registry_len(&self) -> usize {
        self.columns().infantry_registry.len()
    }

    fn remove_infantry_index(&mut self, id: u64) {
        if let Ok(index) = self.cols.infantry_registry.binary_search(&id) {
            self.cols.infantry_registry.remove(index);
        }
    }

    /// Iterate all entities in deterministic stable_id order (immutable).
    pub fn iter_sorted(
        &self,
    ) -> impl DoubleEndedIterator<Item = (u64, &GameEntity)> + ExactSizeIterator {
        self.stored()
    }

    /// [`Self::iter_sorted`] from the first id not below `after`, reached
    /// without walking those before it.
    pub(crate) fn iter_sorted_from(&self, after: u64) -> impl Iterator<Item = (u64, &GameEntity)> {
        self.check_attached();
        let cols = self.columns();
        let overlay = self.written_overlay();
        let start = cols.ids.partition_point(|&id| id < after);
        cols.ids[start..]
            .iter()
            .zip(&cols.entities[start..])
            .filter_map(move |(&id, entity)| {
                let entity = entity.as_deref()?;
                Some((
                    id,
                    overlay.map_or(entity, |overlay| overlay.entity_of(id, entity)),
                ))
            })
    }

    /// Iterate all entity values in deterministic stable_id order (immutable).
    pub fn values_sorted(&self) -> impl ExactSizeIterator<Item = &GameEntity> {
        self.stored().map(|(_, entity)| entity)
    }

    /// Iterate all entities in stable_id order (immutable).
    pub fn values(&self) -> impl ExactSizeIterator<Item = &GameEntity> {
        self.stored().map(|(_, entity)| entity)
    }

    /// The entities of `category` in stable_id order, chosen by the category
    /// column without loading the others.
    pub fn values_of_category(
        &self,
        category: EntityCategory,
    ) -> impl Iterator<Item = &GameEntity> {
        self.check_attached();
        let cols = self.columns();
        let overlay = self.written_overlay();
        cols.categories
            .iter()
            .zip(&cols.entities)
            .enumerate()
            .filter(move |&(_, (&stored, _))| stored == category)
            .filter_map(move |(slot, (_, entity))| {
                let entity = entity.as_deref()?;
                Some(overlay.map_or(entity, |overlay| overlay.entity_of(cols.ids[slot], entity)))
            })
    }

    /// The entities `keep` selects by id and owner, in stable_id order,
    /// chosen from the id and owner columns without loading the others.
    /// Debug builds check each owner against its entity (the live-read
    /// check).
    pub(crate) fn values_of_owners(
        &self,
        mut keep: impl FnMut(u64, InternedId) -> bool,
    ) -> impl Iterator<Item = &GameEntity> {
        let cols = self.columns();
        let overlay = self.written_overlay();
        cols.ids
            .iter()
            .zip(&cols.owners)
            .zip(&cols.entities)
            .filter_map(move |((&id, &owner), entity)| {
                let entity = entity.as_deref()?;
                let entity = overlay.map_or(entity, |overlay| overlay.entity_of(id, entity));
                debug_assert!(
                    !crate::sim::touch_log::live_read_check_enabled() || entity.owner() == owner,
                    "the owner column disagrees with entity {id}"
                );
                keep(id, owner).then_some(entity)
            })
    }

    /// Iterate all entities mutably in stable_id order.
    pub(super) fn values_mut(&mut self) -> impl Iterator<Item = &mut GameEntity> {
        self.own_columns();
        self.touched.note_all();
        self.looks.note_all();
        self.wake_all();
        self.cols.entities.iter_mut().filter_map(entity_mut)
    }

    pub(super) fn hold_parachute_animation(&mut self, holder: u64, anim: u64) -> Option<()> {
        self.write_held_anim(holder, |entity, authority| {
            entity.hold_parachute_animation(anim, authority)
        })
    }

    pub(super) fn hold_emp_sparkle(&mut self, holder: u64, anim: u64) -> Option<()> {
        self.write_held_anim(holder, |entity, authority| {
            entity.hold_emp_sparkle(anim, authority)
        })
    }

    /// Return the Anim replaced at `index`, if the effect held one.
    pub(super) fn hold_attached_effect_anim(
        &mut self,
        holder: u64,
        index: usize,
        anim: u64,
    ) -> Option<u64> {
        self.write_held_anim(holder, |entity, authority| {
            entity.hold_attached_effect_anim(index, anim, authority)
        })
        .flatten()
    }

    pub(super) fn begin_parasite_grapple(&mut self, holder: u64, anim: Option<u64>) -> Option<()> {
        self.write_held_anim(holder, |entity, authority| {
            entity.parasite.begin_grapple(anim, authority)
        })
        .flatten()
    }

    /// Store an Anim in one of `holder`'s fields [`GameEntity::held_anims`]
    /// names through `write`, that field's own write, then note every Anim
    /// the holder holds for [`Self::take_anim_holders`]. The only route to
    /// such a store: those writes take an [`AnimHolderAuthority`], which
    /// only this module makes. Clearing a field needs no note. `None` when
    /// `holder` is not stored.
    fn write_held_anim<R>(
        &mut self,
        holder: u64,
        write: impl FnOnce(&mut GameEntity, AnimHolderAuthority) -> R,
    ) -> Option<R> {
        if self.view.is_some() {
            if !self.contains(holder) {
                return None;
            }
            self.materialise();
        }
        let slot = self.slot_in(&self.cols.slot_of, holder)?;
        self.touched.note(holder, self.cols.slot_of.len());
        self.wake_slot(slot as usize);
        let entity = entity_mut(&mut self.cols.entities[slot as usize])?;
        // A held Anim is one of the names the pointer-expiry index reads.
        let _ = entity.note_names();
        let written = write(entity, AnimHolderAuthority(()));
        self.cols
            .anim_holders
            .extend(entity.held_anims().map(|anim| (anim, holder)));
        Some(written)
    }

    /// The stored entities that may hold `anim` in a field
    /// [`GameEntity::held_anims`] names, ascending, leaving none noted for
    /// it: the caller, `Simulation::notify_anim_pointer_expired`, clears
    /// every such field naming it. Debug builds check that every stored
    /// entity holding it is among them (the live-read check).
    pub(crate) fn take_anim_holders(&mut self, anim: u64) -> Vec<u64> {
        let noted: Vec<u64> = self
            .columns()
            .anim_holders
            .range((anim, 0)..=(anim, u64::MAX))
            .map(|&(_, holder)| holder)
            .collect();
        // An Anim nothing holds leaves the index as it is.
        if !noted.is_empty() {
            self.own_columns();
            for &holder in &noted {
                self.cols.anim_holders.remove(&(anim, holder));
            }
        }
        let holders: Vec<u64> = noted
            .into_iter()
            .filter(|&holder| self.contains(holder))
            .collect();
        debug_assert!(
            !crate::sim::touch_log::live_read_check_enabled()
                || self
                    .par_select(|_, entity| entity.held_anims().any(|held| held == anim))
                    .iter()
                    .all(|holder| holders.binary_search(holder).is_ok()),
            "an entity holds Anim {anim} without a noted pair"
        );
        holders
    }

    /// `(Anim, holder)` for every Anim a stored entity holds.
    fn walk_anim_holders(&self) -> BTreeSet<(u64, u64)> {
        self.stored()
            .flat_map(|(id, entity)| entity.held_anims().map(move |anim| (anim, id)))
            .collect()
    }

    /// Move an entity to a new owner. It does NOT touch the houses' tracking
    /// or lists; `Simulation::change_owner`, the only production caller,
    /// moves them.
    /// No-op if the entity is absent or already owned by `new_owner`.
    pub fn change_owner(&mut self, stable_id: u64, new_owner: crate::sim::intern::InternedId) {
        self.own_columns();
        self.touched.note(stable_id, self.len());
        if let Some(slot) = self.slot(stable_id) {
            self.wake_slot(slot as usize);
        }
        if let Some(slot) = self.slot(stable_id)
            && let Some(e) = entity_mut(&mut self.cols.entities[slot as usize])
            && e.owner() != new_owner
        {
            let (old_owner, category) = (e.owner(), e.category());
            e.set_owner_from_store(new_owner, OwnerChangeAuthority(()));
            self.cols.owners[slot as usize] = new_owner;
            self.unlist_owned(stable_id, (old_owner, category));
            self.list_owned(stable_id, (new_owner, category));
            self.looks.note(stable_id, self.cols.slot_of.len());
        }
    }

    /// The stored ids of `owner`'s objects of `category`, ascending: the ids
    /// a walk of every entity in stable_id order keeps for that owner and
    /// category, without loading the others. Debug builds check them against
    /// that walk (the live-read check).
    pub(crate) fn owned_ids(&self, owner: InternedId, category: EntityCategory) -> &[u64] {
        let ids = self
            .columns()
            .owned
            .get(&(owner, category))
            .map_or(&[][..], Vec::as_slice);
        debug_assert!(
            !crate::sim::touch_log::live_read_check_enabled()
                || ids.iter().copied().eq(self
                    .stored()
                    .filter(|(_, entity)| entity.owner() == owner && entity.category() == category)
                    .map(|(id, _)| id)),
            "the owned index disagrees with the entities of {owner:?}'s {category:?}"
        );
        ids
    }

    /// Put `id` on the `key` owner's list, in order. Ids mostly arrive
    /// ascending, so this mostly appends.
    fn list_owned(&mut self, id: u64, key: (InternedId, EntityCategory)) {
        let ids = self.cols.owned.entry(key).or_default();
        if let Err(index) = ids.binary_search(&id) {
            ids.insert(index, id);
        }
    }

    /// Take `id` off the `key` owner's list.
    fn unlist_owned(&mut self, id: u64, key: (InternedId, EntityCategory)) {
        if let Some(ids) = self.cols.owned.get_mut(&key)
            && let Ok(index) = ids.binary_search(&id)
        {
            ids.remove(index);
        }
    }

    /// Write `stable_id`'s AircraftTracker membership, its bucket (`None`:
    /// not tracked) and enter order, and the airborne set with it. The
    /// tracker's Add, Remove and Update (`Simulation::aircraft_tracker_add`
    /// and its siblings) are the only callers. No-op if the entity is absent.
    pub(crate) fn set_air_membership(
        &mut self,
        stable_id: u64,
        bucket: Option<u16>,
        enter_order: u64,
    ) {
        if self.view.is_some() {
            if !self.contains(stable_id) {
                return;
            }
            self.materialise();
        }
        let Some(slot) = self.slot_in(&self.cols.slot_of, stable_id) else {
            return;
        };
        self.touched.note(stable_id, self.cols.slot_of.len());
        self.wake_slot(slot as usize);
        let Some(entity) = entity_mut(&mut self.cols.entities[slot as usize]) else {
            return;
        };
        if let Some(entry) = airborne_entry(entity) {
            self.cols.airborne.remove(&entry);
        }
        entity.set_air_membership_from_store(bucket, enter_order, AirMembershipAuthority(()));
        if let Some(bucket) = bucket {
            self.cols.airborne.insert((bucket, enter_order, stable_id));
        }
    }

    /// The ids the AircraftTracker holds, in no order a caller may rely on.
    pub(crate) fn airborne_ids(&self) -> impl Iterator<Item = u64> + '_ {
        self.columns().airborne.iter().map(|&(_, _, id)| id)
    }

    /// The stored entities the AircraftTracker holds, by enter order and,
    /// for the ties only fixtures make, stable_id.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn airborne(&self) -> impl Iterator<Item = &GameEntity> {
        let mut entries: Vec<(u64, u64)> = self
            .columns()
            .airborne
            .iter()
            .map(|&(_, order, id)| (order, id))
            .collect();
        entries.sort_unstable();
        entries.into_iter().filter_map(|(_, id)| self.get(id))
    }

    /// AircraftTracker412B40/4137A0's copy, in retained per-bucket entry
    /// order. Shared by GreatestThreat6F91D3 and area damage4892D8. Current
    /// object coordinates do not relocate entries; FootAir owns that update.
    /// Repeated query buckets deliberately copy their vectors again. Each
    /// bucket's entries come from the airborne set, which debug builds check
    /// against a walk of every entity on every call (the live-read check).
    /// The ids are read as the caller takes them, from the store it borrows,
    /// so the copy needs no list of its own.
    pub(crate) fn airborne_in_query_order(
        &self,
        center_cell: &crate::sim::cell_rect::CellRef<'_>,
        radius_cells: i32,
        map_extent: (u16, u16),
    ) -> impl Iterator<Item = u64> + use<'_> {
        //412B6E calls Cell vt+48;413A30 truncates the returned XYZ's
        // X/Y toward zero and packs16. Read the resolved live Cell rather
        // than its lookup argument: aliases and the shared Dummy can differ.
        let point = center_cell
            .center_coordinate()
            .expect("admitted Cell ground coordinate");
        let center = (
            crate::util::lepton::lepton_to_cell_packed(point.x) as u16,
            crate::util::lepton::lepton_to_cell_packed(point.y) as u16,
        );
        let airborne = &self.columns().airborne;
        debug_assert!(
            !crate::sim::touch_log::live_read_check_enabled() || *airborne == self.walk_airborne(),
            "the airborne set disagrees with the entities"
        );
        crate::sim::occupancy::air_spatial_query_bucket_order(
            center.0,
            center.1,
            radius_cells,
            map_extent.0,
            map_extent.1,
        )
        .flat_map(move |bucket| {
            airborne
                .range((bucket, 0, 0)..=(bucket, u64::MAX, u64::MAX))
                .map(|&(_, _, id)| id)
        })
    }

    /// `(bucket, enter order, stable_id)` of every stored entity the tracker
    /// holds.
    fn walk_airborne(&self) -> BTreeSet<(u16, u64, u64)> {
        self.values().filter_map(airborne_entry).collect()
    }

    /// Rebuild class registries, and the owners' lists, from primary storage.
    /// Owned by Deserialize; synthetic fixtures may also rebuild after raw
    /// setup.
    pub(crate) fn rebuild_class_registries(&mut self) {
        self.own_columns();
        let mut owned: LookupMap<(InternedId, EntityCategory), Vec<u64>> = LookupMap::default();
        // Storage iterates in ascending stable_id order, so each list comes
        // out sorted.
        for (id, entity) in self.stored() {
            owned
                .entry((entity.owner(), entity.category()))
                .or_default()
                .push(id);
        }
        self.cols.owned = owned;
        let infantry: Vec<u64> = self
            .stored()
            .filter(|(_, entity)| entity.category() == EntityCategory::Infantry)
            .map(|(id, _)| id)
            .collect();
        // Storage iterates in ascending stable_id order, so the infantry
        // registry comes out sorted.
        self.cols.infantry_registry = infantry;
        self.cols.foot_registry = self
            .stored()
            .filter(|(_, entity)| entity.category().is_foot())
            .map(|(id, _)| id)
            .collect();
        self.cols.techno_registry = self.stored().map(|(id, _)| id).collect();
        self.cols.building_registry = self
            .stored()
            .filter(|(_, entity)| entity.category() == EntityCategory::Structure)
            .map(|(id, _)| id)
            .collect();
    }
}

/// Forks (VERA-only, sim-scale; see the module documentation).
#[cfg_attr(
    not(any(test, feature = "test-support")),
    expect(dead_code, reason = "no pass runs object turns on forks yet")
)]
impl EntityStore {
    /// VERA-only (sim-scale): make this store the frozen base its
    /// [`Self::fork`]s read. The columns move behind one `Arc`, uncopied; the
    /// store answers every read as before and refuses writes until
    /// [`Self::unshare`].
    pub(crate) fn share(&mut self) {
        assert!(
            self.view.is_none() && self.set_aside.is_none(),
            "share of an entity store that is shared or a fork"
        );
        let base = Arc::new(std::mem::take(&mut self.cols));
        self.view = Some(Box::new(View {
            base,
            fork: false,
            overlay: Overlay::default(),
        }));
    }

    /// Own the columns again after [`Self::share`]. Panics while a fork of
    /// them is alive.
    pub(crate) fn unshare(&mut self) {
        assert!(
            self.view.as_deref().is_some_and(|view| !view.fork),
            "unshare of an entity store that is not shared"
        );
        let view = self.view.take().expect("checked above");
        self.cols = Arc::try_unwrap(view.base).unwrap_or_else(|_| {
            panic!("unshare of an entity store with a fork still alive: drop every fork first")
        });
    }

    /// A store that reads this shared one ([`Self::share`]) and keeps its
    /// own writes, for one object's turn at a time
    /// ([`Self::take_fork_writes`], [`Self::reset_fork`]). Making one copies
    /// no entity and counts no reference per entity; it starts focused as
    /// this store is, with touch logs as a clone's.
    pub(crate) fn fork(&self) -> EntityStore {
        let view = self
            .view
            .as_deref()
            .filter(|view| !view.fork)
            .expect("fork of an entity store that is not shared");
        EntityStore {
            cols: Columns::default(),
            view: Some(Box::new(View {
                base: Arc::clone(&view.base),
                fork: true,
                overlay: Overlay {
                    focus: Written::new(self.focus.0),
                    ..Overlay::default()
                },
            })),
            set_aside: None,
            focus: self.focus,
            #[cfg(any(test, feature = "test-support"))]
            lookups: Default::default(),
            touched: TouchLogs::everything(),
            looks: TouchLogs::everything(),
            detached: self.detached,
            pass: 0,
        }
    }

    /// What this fork wrote since its last take or reset, leaving it as
    /// [`Self::reset_fork`] does.
    pub(crate) fn take_fork_writes(&mut self) -> EntityWrites {
        let writes = match self.view.as_deref_mut() {
            Some(view) if view.fork => EntityWrites {
                overlay: std::mem::take(&mut view.overlay),
                structural: false,
            },
            None if self.set_aside.is_some() => EntityWrites {
                overlay: Overlay::default(),
                structural: true,
            },
            _ => panic!("take_fork_writes of an entity store that is not a fork"),
        };
        self.reset_fork();
        writes
    }

    /// Drop what this fork wrote, its own columns if it met a structural
    /// write, and its touch logs: it reads its base again. Costs what the
    /// fork wrote.
    pub(crate) fn reset_fork(&mut self) {
        if let Some(view) = self.set_aside.take() {
            self.cols = Columns::default();
            self.view = Some(view);
            self.refocus();
        }
        let focus = self.focus.0;
        self.view
            .as_deref_mut()
            .filter(|view| view.fork)
            .expect("reset_fork of an entity store that is not a fork")
            .overlay
            .clear(focus);
        self.touched = TouchLogs::everything();
        self.looks = TouchLogs::everything();
    }

    /// Apply one turn's writes from a fork ([`Self::take_fork_writes`]) to
    /// this store, which shared the fork's base and has been unshared since:
    /// each written entity takes its slot (the copy moves in; nothing is
    /// cloned) and each written sleep cell its cell, the touch and look logs
    /// note them as the turn's hand-outs and [`Self::note_look`] calls would
    /// have, and every written slot is stamped for
    /// [`Self::written_this_pass`]. A write outside the structural writers
    /// changes no index, which debug builds check. Structural writes are
    /// refused: the caller runs such a turn again on this store.
    pub(crate) fn commit_writes(&mut self, writes: EntityWrites) {
        assert!(
            self.view.is_none() && self.set_aside.is_none(),
            "commit_writes to an entity store that is shared or a fork"
        );
        debug_assert!(
            !writes.structural,
            "a structural turn's writes cannot be merged: run the turn again"
        );
        if writes.structural {
            return;
        }
        let stored = self.cols.slot_of.len();
        let Overlay {
            focus,
            others,
            looks,
        } = writes.overlay;
        for written in std::iter::once(focus)
            .chain(others)
            .filter(Written::is_written)
        {
            let Some(slot) = self.committed_slot(&written) else {
                continue;
            };
            if let Some(entity) = written.entity {
                debug_assert!(
                    self.cols.entities[slot]
                        .as_deref()
                        .is_some_and(|old| self.indexes_hold(old, &entity)),
                    "a merged write changed what an index of entity {} derives from",
                    written.id
                );
                self.cols.entities[slot] = Some(entity);
            }
            if let Some(cell) = written.cell {
                self.cols.sleep[slot] = cell;
            }
            self.cols.sleep[slot].written = self.pass;
            if written.noted {
                self.note_hand_out(written.id, slot);
            }
        }
        for id in looks {
            self.looks.note(id, stored);
        }
    }

    /// `written`'s slot in this store: where the fork found it, unless a
    /// rerun moved it since.
    fn committed_slot(&self, written: &Written) -> Option<usize> {
        let slot = written.slot as usize;
        if self.cols.ids.get(slot) == Some(&written.id) && self.cols.entities[slot].is_some() {
            return Some(slot);
        }
        let slot = self
            .cols
            .slot_of
            .get(&written.id)
            .map(|&slot| slot as usize);
        debug_assert!(
            slot.is_some(),
            "commit_writes of entity {}, which the store no longer holds",
            written.id
        );
        slot
    }

    /// Debug builds: `new`, replacing `old`, keeps what the store's indexes
    /// derive from (owner, category, air membership) and holds no Anim the
    /// anim-holder index does not note.
    fn indexes_hold(&self, old: &GameEntity, new: &GameEntity) -> bool {
        old.stable_id() == new.stable_id()
            && old.owner() == new.owner()
            && old.category() == new.category()
            && airborne_entry(old) == airborne_entry(new)
            && new
                .held_anims()
                .all(|anim| self.cols.anim_holders.contains(&(anim, new.stable_id())))
    }

    /// Begin a pass for [`Self::written_this_pass`]: from here every
    /// mutable hand-out, sleep-cell write and [`Self::commit_writes`] stamps
    /// its slot. For the store itself, when its forks' base is taken.
    pub(crate) fn begin_pass(&mut self) {
        assert!(
            self.view.is_none() && self.set_aside.is_none(),
            "begin_pass of an entity store that is shared or a fork"
        );
        self.pass = self.pass.wrapping_add(1);
        if self.pass == 0 {
            // A number come round again could match a stamp 65,535 passes
            // old.
            for cell in &mut self.cols.sleep {
                cell.written = 0;
            }
            self.pass = 1;
        }
    }

    /// Whether `stable_id`'s entity or sleep cell was written since
    /// [`Self::begin_pass`]: handed out for writing (or stepped, woken,
    /// marked) or committed. False before the first pass, for a fork, and
    /// for an id the store does not hold.
    pub(crate) fn written_this_pass(&self, stable_id: u64) -> bool {
        self.slot(stable_id)
            .is_some_and(|slot| self.stamped(slot as usize))
    }

    /// Whether `slot` was written since [`Self::begin_pass`].
    fn stamped(&self, slot: usize) -> bool {
        self.pass != 0 && self.columns().sleep[slot].written == self.pass
    }
}

impl View {
    /// `id`'s entity as the view reads it: the fork's copy, else the base's
    /// in `slot`.
    #[inline]
    fn entity(&self, id: u64, focused: bool, slot: u32) -> Option<&GameEntity> {
        match self
            .overlay
            .written(id, focused)
            .and_then(|written| written.entity.as_deref())
        {
            Some(copy) => Some(copy),
            None => self.base.entities[slot as usize].as_deref(),
        }
    }

    /// `id`'s sleep cell as the view reads it.
    #[inline]
    fn cell(&self, id: u64, focused: bool, slot: u32) -> SleepCell {
        self.overlay
            .written(id, focused)
            .and_then(|written| written.cell)
            .unwrap_or_else(|| self.base.sleep[slot as usize])
    }

    /// `id`'s writes, its entity copied from the base's on first need.
    fn copied(&mut self, id: u64, focused: bool, slot: u32) -> &mut Written {
        let written = self.overlay.written_mut(id, focused, slot);
        if written.entity.is_none() {
            written.entity = self.base.entities[slot as usize]
                .as_deref()
                .cloned()
                .map(Arc::new);
        }
        written
    }

    /// The fork's copy of `id`'s sleep cell, made from the base's on first
    /// need.
    fn cell_mut(&mut self, id: u64, focused: bool, slot: u32) -> &mut SleepCell {
        let base = self.base.sleep[slot as usize];
        self.overlay
            .written_mut(id, focused, slot)
            .cell
            .get_or_insert(base)
    }

    /// [`EntityStore::wake_slot`] in the fork's copies: written only where
    /// waking changes something.
    fn wake(&mut self, id: u64, focused: bool, slot: u32) {
        let cell = self.cell(id, focused, slot);
        if cell.wake_at == 0 {
            if cell.marks != 0 {
                self.cell_mut(id, focused, slot).marks = 0;
            }
            return;
        }
        *self.cell_mut(id, focused, slot) = SleepCell::default();
        if cell.owed != 0
            && let Some(entity) = entity_mut(&mut self.copied(id, focused, slot).entity)
        {
            entity.settle_sleep(cell.owed);
        }
    }
}

/// A fork's view, for a write; panics for a shared store's own.
fn fork_view(view: &mut Option<Box<View>>) -> &mut View {
    let view = view.as_deref_mut().expect("a store reading shared columns");
    assert!(view.fork, "{SHARED_WRITE}");
    view
}

impl Overlay {
    /// `id`'s writes, if any: `focused` says it is the fork's focused id.
    #[inline]
    fn written(&self, id: u64, focused: bool) -> Option<&Written> {
        if focused {
            Some(&self.focus)
        } else if self.others.is_empty() {
            None
        } else {
            self.other(id)
        }
    }

    /// `id`'s writes among the others.
    #[inline(never)]
    fn other(&self, id: u64) -> Option<&Written> {
        self.others.iter().find(|written| written.id == id)
    }

    /// `id`'s writes, an empty entry made on first need, which `slot` holds.
    fn written_mut(&mut self, id: u64, focused: bool, slot: u32) -> &mut Written {
        let written = if focused {
            &mut self.focus
        } else if let Some(index) = self.others.iter().position(|written| written.id == id) {
            &mut self.others[index]
        } else {
            self.others.push(Written::new(id));
            self.others.last_mut().expect("just pushed")
        };
        written.slot = slot;
        written
    }

    /// `id`'s writes, found by id alone.
    fn by_id(&self, id: u64) -> Option<&Written> {
        self.written(id, self.focus.id == id)
    }

    /// The fork's copy of `id`'s entity, else `entity`, the base's.
    fn entity_of<'a>(&'a self, id: u64, entity: &'a GameEntity) -> &'a GameEntity {
        self.by_id(id)
            .and_then(|written| written.entity.as_deref())
            .unwrap_or(entity)
    }

    /// The fork's copy of `id`'s sleep cell, else `cell`, the base's.
    fn cell_of(&self, id: u64, cell: SleepCell) -> SleepCell {
        self.by_id(id)
            .and_then(|written| written.cell)
            .unwrap_or(cell)
    }

    /// Whether an entity or a sleep cell was written.
    fn is_written(&self) -> bool {
        self.focus.is_written() || !self.others.is_empty()
    }

    /// Every id's writes that hold something.
    fn records(&self) -> impl Iterator<Item = &Written> {
        std::iter::once(&self.focus)
            .chain(&self.others)
            .filter(|written| written.is_written())
    }

    /// Move the focused entry to `id`, keeping what was written of the one
    /// focused before.
    fn refocus(&mut self, id: u64) {
        let focus = match self.others.iter().position(|written| written.id == id) {
            Some(index) => self.others.swap_remove(index),
            None => Written::new(id),
        };
        let before = std::mem::replace(&mut self.focus, focus);
        if before.is_written() {
            self.others.push(before);
        }
    }

    /// Drop every write, keeping the lists' room, focused on `focus`.
    fn clear(&mut self, focus: u64) {
        self.focus = Written::new(focus);
        self.others.clear();
        self.looks.clear();
    }

    /// Put the written entities and cells in their slots of `cols`.
    fn apply(self, cols: &mut Columns) {
        for written in std::iter::once(self.focus).chain(self.others) {
            let slot = written.slot as usize;
            if let Some(entity) = written.entity {
                cols.entities[slot] = Some(entity);
            }
            if let Some(cell) = written.cell {
                cols.sleep[slot] = cell;
            }
        }
    }
}

impl Written {
    fn new(id: u64) -> Self {
        Self {
            id,
            ..Self::default()
        }
    }

    /// Whether the entity or the sleep cell was written.
    fn is_written(&self) -> bool {
        self.entity.is_some() || self.cell.is_some()
    }
}

/// [`EntityStore`]'s stored entities in ascending stable_id order, skipping
/// empty slots, a fork's copies in place of its base's. Its length is
/// exact, so a collect allocates once.
struct Stored<'a> {
    ids: std::slice::Iter<'a, u64>,
    entities: std::slice::Iter<'a, Option<Arc<GameEntity>>>,
    /// Stored entities not yet yielded from either end.
    remaining: usize,
    /// A fork's writes, if it wrote anything.
    overlay: Option<&'a Overlay>,
}

impl<'a> Stored<'a> {
    fn yielded(&mut self, id: u64, entity: &'a GameEntity) -> (u64, &'a GameEntity) {
        self.remaining -= 1;
        let entity = self
            .overlay
            .map_or(entity, |overlay| overlay.entity_of(id, entity));
        (id, entity)
    }
}

impl<'a> Iterator for Stored<'a> {
    type Item = (u64, &'a GameEntity);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let id = *self.ids.next()?;
            if let Some(entity) = self.entities.next()?.as_deref() {
                return Some(self.yielded(id, entity));
            }
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl DoubleEndedIterator for Stored<'_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        loop {
            let id = *self.ids.next_back()?;
            if let Some(entity) = self.entities.next_back()?.as_deref() {
                return Some(self.yielded(id, entity));
            }
        }
    }
}

impl ExactSizeIterator for Stored<'_> {}

/// One stored entity of an [`EntityStore::for_each_entry_mut`] walk. It holds
/// the store, so the visit can neither add nor remove an entity: the slot
/// stays filled for as long as the entry lives.
pub(super) struct EntryMut<'a> {
    store: &'a mut EntityStore,
    slot: usize,
}

impl<'a> EntryMut<'a> {
    pub(super) fn id(&self) -> u64 {
        self.store.columns().ids[self.slot]
    }

    /// Read the entity without handing it out.
    pub(super) fn get(&self) -> &GameEntity {
        self.store
            .entity_at(self.id(), self.slot as u32)
            .expect("the walk visits filled slots only")
    }

    /// Hand the entity out for writing, as [`EntityStore::get_mut`] does:
    /// noted in the touch logs and woken first.
    pub(super) fn into_mut(self) -> &'a mut GameEntity {
        let id = self.id();
        let store = self.store;
        if store.view.is_some() {
            return store
                .fork_entity_mut(id, ForkWrite::HandOut)
                .expect("the walk visits filled slots only");
        }
        store.note_hand_out(id, self.slot);
        store.wake_slot(self.slot);
        entity_mut(&mut store.cols.entities[self.slot]).expect("the walk visits filled slots only")
    }
}

/// A descending walk of an [`EntityStore`]'s stored ids
/// ([`EntityStore::descending_ids`]). Each step looks up the highest stored id
/// below the last one it gave, so between steps the store may change:
///
/// - An entity removed before the walk reaches it is not visited, as a
///   snapshot walk's lookup of it finds nothing.
/// - An entity stored during the walk has a fresh id, above every id the
///   store held ([`crate::sim::world::StableIdCursor::allocate`] counts
///   up and a load refuses a counter behind the highest id), so the walk,
///   which only goes down, never visits it: again as a snapshot taken at
///   the start would not. This holds for allocated ids only; a test that
///   stores hand-picked ids with gaps can be handed an id in a gap.
/// - Compaction moves slots, not ids, and the cursor is an id.
///
/// The walk therefore visits exactly the ids a copy of the stored ids taken
/// at its start would, in the same order, less those removed before their
/// turn.
#[derive(Debug, Clone, Copy)]
pub(crate) struct DescendingIds {
    /// The last id given; `None` before the first step.
    below: Option<u64>,
    keep: fn(EntityCategory) -> bool,
}

impl DescendingIds {
    /// The next stored id down whose category the walk keeps.
    pub(crate) fn next(&mut self, store: &EntityStore) -> Option<u64> {
        store.check_attached();
        let cols = store.columns();
        let end = self.below.map_or(cols.ids.len(), |below| {
            cols.ids.partition_point(|&id| id < below)
        });
        let slot = (0..end)
            .rev()
            .find(|&slot| cols.entities[slot].is_some() && (self.keep)(cols.categories[slot]))?;
        let id = cols.ids[slot];
        self.below = Some(id);
        Some(id)
    }
}

impl serde::Serialize for EntityStore {
    /// The stored entities as a map from stable_id, in ascending order: the
    /// form the former `BTreeMap` storage wrote.
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        // The save codec needs the length up front, which a filtering iterator
        // cannot give `collect_map`.
        let mut map = serializer.serialize_map(Some(self.len()))?;
        for (id, entity) in self.stored() {
            // A sleeping entity is saved as its next turn would find it.
            match self.owed_visits(id) {
                0 => map.serialize_entry(&id, entity)?,
                owed => {
                    let mut settled = entity.clone();
                    settled.settle_sleep(owed);
                    map.serialize_entry(&id, &settled)?;
                }
            }
        }
        map.end()
    }
}

impl<'de> serde::Deserialize<'de> for EntityStore {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let entities = BTreeMap::<u64, Box<GameEntity>>::deserialize(deserializer)?;
        let mut store = Self::new();
        let cols = &mut store.cols;
        cols.ids.reserve(entities.len());
        cols.entities.reserve(entities.len());
        for (id, entity) in entities {
            cols.slot_of.insert(id, cols.ids.len() as u32);
            cols.ids.push(id);
            cols.owners.push(entity.owner());
            cols.categories.push(entity.category());
            cols.sleep.push(SleepCell::default());
            cols.entities.push(Some(Arc::from(entity)));
        }
        store.refocus();
        store.rebuild_class_registries();
        store.cols.airborne = store.walk_airborne();
        store.cols.anim_holders = store.walk_anim_holders();
        Ok(store)
    }
}

impl Default for EntityStore {
    fn default() -> Self {
        Self::new()
    }
}

/// A stored entity for writing. Only a fork's own copy of the columns shares
/// entities ([`EntityStore::materialise`]), so only there does this copy the
/// entity it hands out; elsewhere it checks the count and hands it out.
#[inline]
fn entity_mut(entity: &mut Option<Arc<GameEntity>>) -> Option<&mut GameEntity> {
    entity.as_mut().map(Arc::make_mut)
}

/// `entity`'s entry in the airborne set, if the AircraftTracker holds it.
fn airborne_entry(entity: &GameEntity) -> Option<(u16, u64, u64)> {
    entity
        .air_spatial_bucket()
        .map(|bucket| (bucket, entity.air_spatial_enter_order(), entity.stable_id()))
}

#[cfg(all(test, tests_sim_c_g))]
mod tests {
    use super::*;
    use crate::sim::game_entity::GameEntity;

    /// An AI visit steps the counter without handing the entity out, and a
    /// refused `get_mut_if` hands nothing out either.
    #[test]
    fn an_ai_visit_hands_nothing_out() {
        let mut store = EntityStore::new();
        store.insert(GameEntity::test_default(7, "MTNK", "Americans", 5, 5));
        let handed_out = store.hand_outs();
        store.count_mission_ai_visit(7);
        store.count_mission_ai_visit(7);
        assert!(store.get_mut_if(7, |_| false).is_none());
        assert_eq!(store.hand_outs(), handed_out);
        assert_eq!(store.get(7).unwrap().mission.ai_counter(), 2);
    }

    /// Warming a record is no lookup, no hand-out and no wake, whether the id
    /// is stored, asleep or absent, and the focused id stays focused.
    #[test]
    fn warming_a_record_reads_and_wakes_nothing() {
        let mut store = EntityStore::new();
        for id in [3, 5] {
            store.insert(GameEntity::test_default(id, "MTNK", "Americans", 5, 5));
        }
        store.sleep_until(5, 100, 0);
        store.focus(3);
        let (lookups, handed_out) = (store.lookups(), store.hand_outs());
        for id in [3, 5, 9] {
            store.warm(id);
        }
        assert_eq!(store.lookups(), lookups);
        assert_eq!(store.hand_outs(), handed_out);
        assert_eq!(store.sleeps_until(5), Some((100, 0)));
        let by_index = store.lookups().1;
        assert!(store.get(3).is_some());
        assert_eq!(store.lookups().1, by_index, "3 is still the focused id");
    }

    /// The entry walk skips removed slots and unkept categories, and only an
    /// entry taken for writing is handed out and woken, as `get_mut` would.
    #[test]
    fn the_entry_walk_hands_out_only_entries_taken_for_writing() {
        let mut store = EntityStore::new();
        for id in [3, 5, 8, 9] {
            store.insert(GameEntity::test_default(id, "MTNK", "Americans", 5, 5));
        }
        store.insert(GameEntity::test_default_of_category(
            11,
            "GI",
            "Americans",
            5,
            5,
            EntityCategory::Infantry,
        ));
        store.remove(5);
        store.sleep_until(3, 100, 0);
        store.sleep_until(8, 100, 0);
        let handed_out = store.hand_outs();
        let mut visited = Vec::new();
        store.for_each_entry_mut(
            |category| category == EntityCategory::Unit,
            |entry| {
                visited.push(entry.id());
                if entry.id() == 8 {
                    entry.into_mut();
                }
            },
        );
        assert_eq!(visited, [3, 8, 9]);
        assert_eq!(store.hand_outs() - handed_out, 1);
        assert_eq!(
            store.sleeps_until(3),
            Some((100, 0)),
            "only read: still asleep"
        );
        assert_eq!(store.sleeps_until(8), None, "taken for writing: woken");
    }

    /// The descending cursor visits what a copy of the ids taken at its
    /// start would, in the same order: an entity removed before its turn is
    /// passed over, one stored during the walk (a fresh, higher id) is not
    /// visited, unkept categories are skipped, and a compaction mid-walk
    /// loses nothing.
    #[test]
    fn the_descending_cursor_walks_as_a_snapshot_would() {
        let mut store = EntityStore::new();
        for id in 1..=100 {
            store.insert(GameEntity::test_default(id, "MTNK", "Americans", 5, 5));
        }
        store.insert(GameEntity::test_default_of_category(
            101,
            "GI",
            "Americans",
            5,
            5,
            EntityCategory::Infantry,
        ));
        let mut visited = Vec::new();
        let mut cursor = EntityStore::descending_ids(|category| category == EntityCategory::Unit);
        while let Some(id) = cursor.next(&store) {
            visited.push(id);
            if id == 90 {
                // Enough removals below the cursor to compact the store.
                for removed in 2..80 {
                    store.remove(removed);
                }
                store.insert(GameEntity::test_default(200, "MTNK", "Americans", 5, 5));
            }
        }
        assert!(store.cols.ids.len() < 100, "the walk crossed a compaction");
        let expected: Vec<u64> = (80..=100).rev().chain([1]).collect();
        assert_eq!(visited, expected);
    }

    /// Populated original412B40 ->4137A0 controls. Buckets hold opaque
    /// tokens; the last numeric row deliberately repeats a token in two
    /// buckets, which is not a legal EntityStore membership. Its bucket walk
    /// is still compared, without fabricating two actors with the same ID.
    #[test]
    fn shared_air_query_bucket_and_entity_order_match_original() {
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/distributed_fire.json",
        ))
        .unwrap();
        let rows = corpus["air_query"].as_array().unwrap();
        assert_eq!(rows.len(), 7);
        let mut materialized = 0;
        for row in rows {
            let input = &row["input"];
            let point = |name: &str| {
                (
                    input[name][0].as_u64().unwrap() as u16,
                    input[name][1].as_u64().unwrap() as u16,
                )
            };
            let center = point("center_cell");
            let extent = point("map_extent");
            let radius = input["radius_cells"].as_i64().unwrap() as i32;
            let actual: Vec<_> = crate::sim::occupancy::air_spatial_query_bucket_order(
                center.0, center.1, radius, extent.0, extent.1,
            )
            .collect();
            let expected: Vec<_> = row["copied_bucket_order"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_u64().unwrap() as u16)
                .collect();
            assert_eq!(actual, expected, "{}", row["name"]);
            if row["name"] == "retained_order_duplicate_tokens" {
                continue;
            }
            let mut store = EntityStore::new();
            let mut order = 0;
            for bucket in input["buckets"].as_array().unwrap() {
                for token in bucket["ids"].as_array().unwrap() {
                    order += 1;
                    let id = token.as_u64().unwrap();
                    store.insert(make_entity(id).with_air_spatial_membership_for_test(
                        Some(bucket["bucket"].as_u64().unwrap() as u16),
                        order,
                    ));
                }
            }
            let expected: Vec<_> = row["copied_ids"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_u64().unwrap())
                .collect();
            let cell = crate::map::resolved_terrain::test_flat_cell(center.0, center.1);
            assert_eq!(
                store
                    .airborne_in_query_order(
                        &crate::sim::cell_rect::CellRef::Real(&cell),
                        radius,
                        extent
                    )
                    .collect::<Vec<_>>(),
                expected,
                "{}",
                row["name"]
            );
            assert_eq!(row["copied_ids"], row["drained_ids"]);
            assert_eq!(row["remaining_count"], 0);
            materialized += 1;
        }
        assert_eq!(materialized, 6);
    }

    fn make_entity(id: u64) -> GameEntity {
        GameEntity::test_default(id, "HTNK", "Americans", 10, 10)
    }

    /// Each reader takes its own log, so one reader's take leaves the other's
    /// notes in place; a clone reports everything to both.
    #[test]
    fn each_touch_reader_takes_its_own_log() {
        let mut store = EntityStore::new();
        for id in [1, 2, 3] {
            store.insert(make_entity(id));
        }
        // Nothing has been derived from a fresh store yet.
        assert_eq!(store.take_touched(TouchReader::BlockerPlane), Touched::All);
        assert_eq!(
            store.take_touched(TouchReader::ExpiryListeners),
            Touched::All
        );
        {
            let position = &mut store.get_mut(2).unwrap().position;
            position.set_rx_for_test(position.rx() + 1);
        }
        store.get_mut(3);
        assert_eq!(
            store.take_touched(TouchReader::BlockerPlane),
            Touched::Ids(vec![2, 3])
        );
        store.get_mut(1);
        assert_eq!(
            store.take_touched(TouchReader::ExpiryListeners),
            Touched::Ids(vec![2, 3, 1])
        );
        assert_eq!(
            store.take_touched(TouchReader::BlockerPlane),
            Touched::Ids(vec![1])
        );
        let mut copy = store.clone();
        assert_eq!(
            copy.take_touched(TouchReader::ExpiryListeners),
            Touched::All
        );
        assert_eq!(copy.take_touched(TouchReader::BlockerPlane), Touched::All);
        let _ = store.values_mut().count();
        assert_eq!(
            store.take_touched(TouchReader::ExpiryListeners),
            Touched::All
        );
        assert_eq!(store.take_touched(TouchReader::BlockerPlane), Touched::All);
    }

    /// A narrowed log names only the buildings handed out, and a removal,
    /// while the other reader's names every hand-out; widened, it names every
    /// hand-out again.
    #[test]
    fn a_narrowed_touch_log_names_only_buildings() {
        let mut store = EntityStore::new();
        store.insert(make_entity(1));
        store.insert(GameEntity::test_default_of_category(
            2,
            "GAPOWR",
            "Americans",
            10,
            10,
            EntityCategory::Structure,
        ));
        store.insert(make_entity(3));
        let _ = store.take_touched(TouchReader::BlockerPlane);
        let _ = store.take_touched(TouchReader::ExpiryListeners);
        store.narrow_touched(TouchReader::BlockerPlane, true);
        store.get_mut(1);
        store.get_mut(2);
        let _ = store.get_mut_if(3, |_| true);
        store.remove(3);
        assert_eq!(
            store.take_touched(TouchReader::BlockerPlane),
            Touched::Ids(vec![2, 3])
        );
        assert_eq!(
            store.take_touched(TouchReader::ExpiryListeners),
            Touched::Ids(vec![1, 2, 3])
        );
        store.narrow_touched(TouchReader::BlockerPlane, false);
        store.get_mut(1);
        assert_eq!(
            store.take_touched(TouchReader::BlockerPlane),
            Touched::Ids(vec![1])
        );
    }

    /// Insert, remove and lookup answer as the former `BTreeMap` storage did,
    /// across compactions, re-stored ids and ids below the newest; the owner
    /// column follows every entity's owner through them and through
    /// `change_owner`, the category column every entity's category through
    /// them (and walks of one category keep the stable_id order), each
    /// owner's list of its objects of a category both, in stable_id order,
    /// and the airborne set every entity's tracker membership through them
    /// and through `set_air_membership`.
    #[test]
    fn storage_answers_as_an_ordered_map_through_compactions() {
        let mut store = EntityStore::new();
        let mut model: BTreeMap<u64, u16> = BTreeMap::new();
        let houses = ["Americans", "Russians", "YuriCountry"].map(crate::sim::intern::test_intern);
        let mut owners: BTreeMap<u64, InternedId> = BTreeMap::new();
        let classes = [
            EntityCategory::Unit,
            EntityCategory::Infantry,
            EntityCategory::Structure,
            EntityCategory::Aircraft,
        ];
        let mut categories: BTreeMap<u64, EntityCategory> = BTreeMap::new();
        // Tracked entities' (bucket, enter order). Orders repeat across ids,
        // as fixtures' do, so ties fall to stable_id.
        let mut tracked: BTreeMap<u64, (u16, u64)> = BTreeMap::new();
        let mut seed = 0x2545_f491_4f6c_dd1d_u64;
        let mut next = |bound: u64| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed % bound
        };
        let mut newest = 0;
        let mut compactions = 0;
        let mut largest_compaction = 0;
        for step in 0..20_000 {
            let slots = store.cols.ids.len();
            // 4,000 steps of growth alternate with 2,000 of heavy removal, so
            // the store passes 4,000 slots and compaction runs repeatedly, at
            // small and large sizes.
            let removing = step % 6_000 >= 4_000;
            match next(10) {
                // Mostly appends, as monotonic ids do.
                0..=4 if !removing => {
                    newest += 1 + next(3);
                    let mut entity = make_entity(newest);
                    entity.position.set_rx_for_test(step as u16);
                    entity.set_owner_for_test(houses[step % 3]);
                    entity.set_category_for_test(classes[step % 4]);
                    if next(4) == 0 {
                        let membership = (next(400) as u16, next(1_000));
                        entity = entity
                            .with_air_spatial_membership_for_test(Some(membership.0), membership.1);
                        tracked.insert(newest, membership);
                    }
                    store.insert(entity);
                    model.insert(newest, step as u16);
                    owners.insert(newest, houses[step % 3]);
                    categories.insert(newest, classes[step % 4]);
                }
                // A fixture's id below the newest, or a removed id stored again.
                5 => {
                    let id = next(newest + 1);
                    let mut entity = make_entity(id);
                    entity.position.set_rx_for_test(step as u16);
                    entity.set_owner_for_test(houses[step % 3]);
                    entity.set_category_for_test(classes[step % 4]);
                    // A replacement starts untracked or tracked afresh.
                    tracked.remove(&id);
                    if next(2) == 0 {
                        let membership = (next(400) as u16, next(1_000));
                        entity = entity
                            .with_air_spatial_membership_for_test(Some(membership.0), membership.1);
                        tracked.insert(id, membership);
                    }
                    store.insert(entity);
                    model.insert(id, step as u16);
                    owners.insert(id, houses[step % 3]);
                    categories.insert(id, classes[step % 4]);
                }
                0..=8 => {
                    // While removing, pick a stored id most of the time.
                    let id = if removing && !model.is_empty() && next(4) > 0 {
                        *model.keys().nth(next(model.len() as u64) as usize).unwrap()
                    } else {
                        next(newest + 1)
                    };
                    assert_eq!(store.remove(id).map(|e| e.position.rx()), model.remove(&id));
                    owners.remove(&id);
                    categories.remove(&id);
                    tracked.remove(&id);
                }
                _ => {
                    let id = next(newest + 1);
                    if let Some(entity) = store.get_mut(id) {
                        entity.position.set_rx_for_test(step as u16);
                    }
                    if let Some(rx) = model.get_mut(&id) {
                        *rx = step as u16;
                    }
                    store.change_owner(id, houses[step % 3]);
                    if let Some(owner) = owners.get_mut(&id) {
                        *owner = houses[step % 3];
                    }
                    // The tracker's Add or Update, or its Remove.
                    let membership = (next(3) > 0).then(|| (next(400) as u16, next(1_000)));
                    store.set_air_membership(
                        id,
                        membership.map(|(bucket, _)| bucket),
                        membership.map_or(0, |(_, order)| order),
                    );
                    if model.contains_key(&id) {
                        match membership {
                            Some(membership) => tracked.insert(id, membership),
                            None => tracked.remove(&id),
                        };
                    }
                }
            }
            if store.cols.ids.len() < slots {
                compactions += 1;
                largest_compaction = largest_compaction.max(slots);
            }
            let probe = next(newest + 1);
            assert_eq!(store.contains(probe), model.contains_key(&probe));
            assert_eq!(
                store.get(probe).map(|e| e.position.rx()),
                model.get(&probe).copied()
            );
            assert_eq!(store.owner_of(probe), owners.get(&probe).copied());
            assert_eq!(store.category_of(probe), categories.get(&probe).copied());
            if step % 97 == 0 {
                assert!(
                    owners
                        .iter()
                        .all(|(&id, &owner)| store.owner_of(id) == Some(owner)
                            && store.get(id).map(GameEntity::owner) == Some(owner))
                );
                assert!(
                    categories
                        .iter()
                        .all(|(&id, &category)| store.category_of(id) == Some(category))
                );
                for class in classes {
                    assert_eq!(
                        store
                            .values_of_category(class)
                            .map(GameEntity::stable_id)
                            .collect::<Vec<_>>(),
                        categories
                            .iter()
                            .filter(|&(_, &category)| category == class)
                            .map(|(&id, _)| id)
                            .collect::<Vec<_>>()
                    );
                    for house in houses {
                        assert_eq!(
                            store.owned_ids(house, class),
                            owners
                                .iter()
                                .filter(|&(id, &owner)| owner == house && categories[id] == class)
                                .map(|(&id, _)| id)
                                .collect::<Vec<_>>()
                        );
                    }
                }
                assert_eq!(
                    store.cols.building_registry,
                    categories
                        .iter()
                        .filter(|(_, category)| **category == EntityCategory::Structure)
                        .map(|(&id, _)| id)
                        .collect::<Vec<_>>(),
                    "Building registry derives from the model's immutable class"
                );
                let mut expected_airborne: Vec<(u64, u64, u16)> = tracked
                    .iter()
                    .map(|(&id, &(bucket, order))| (order, id, bucket))
                    .collect();
                expected_airborne.sort_unstable();
                let airborne: Vec<(u64, u64, u16)> = store
                    .airborne()
                    .map(|e| {
                        (
                            e.air_spatial_enter_order(),
                            e.stable_id(),
                            e.air_spatial_bucket().unwrap(),
                        )
                    })
                    .collect();
                assert_eq!(airborne, expected_airborne);
                assert_eq!(store.len(), model.len());
                assert_eq!(
                    store.keys_sorted(),
                    model.keys().copied().collect::<Vec<_>>()
                );
                assert_eq!(
                    store.cols.techno_registry,
                    model.keys().copied().collect::<Vec<_>>(),
                    "native registration rows must not follow dense empty slots"
                );
                let stored: Vec<(u64, u16)> = store
                    .iter_sorted()
                    .rev()
                    .map(|(id, e)| (id, e.position.rx()))
                    .collect();
                let expected: Vec<(u64, u16)> = model.iter().rev().map(|(&k, &v)| (k, v)).collect();
                assert_eq!(stored, expected);
                assert!(
                    store.cols.ids.len() - store.len() < 64
                        || (store.cols.ids.len() - store.len()) * 4 < store.cols.ids.len()
                );
                assert_eq!(store.iter_sorted().len(), model.len());
            }
        }
        assert!(
            compactions >= 5 && largest_compaction >= 2_000,
            "{compactions} compactions, the largest at {largest_compaction} slots"
        );
    }

    /// A save writes the store as the former `BTreeMap` storage did, so the
    /// snapshot format is unchanged, and a restore reads it back in order.
    #[test]
    fn saved_form_is_the_ordered_map_form() {
        let mut store = EntityStore::new();
        for id in [4, 9, 2, 7] {
            store.insert(make_entity(id));
        }
        store.remove(9);
        store.change_owner(7, crate::sim::intern::test_intern("Russians"));
        store.set_air_membership(4, Some(12), 30);
        store.set_air_membership(2, Some(12), 31);
        let map: BTreeMap<u64, Box<GameEntity>> = [2, 4, 7]
            .into_iter()
            .map(|id| (id, Box::new(store.get(id).unwrap().clone())))
            .collect();
        let saved = crate::util::save_codec::serialize(&store).unwrap();
        assert_eq!(saved, crate::util::save_codec::serialize(&map).unwrap());
        let restored: EntityStore = crate::util::save_codec::deserialize(&saved).unwrap();
        assert_eq!(restored.keys_sorted(), [2, 4, 7]);
        assert_eq!(restored.cols.techno_registry, [2, 4, 7]);
        for id in [2, 4, 7] {
            assert_eq!(restored.owner_of(id), Some(store.get(id).unwrap().owner()));
        }
        let (americans, russians) = (
            crate::sim::intern::test_intern("Americans"),
            crate::sim::intern::test_intern("Russians"),
        );
        for copy in [&restored, &store.clone()] {
            assert_eq!(copy.owned_ids(americans, EntityCategory::Unit), [2, 4]);
            assert_eq!(copy.owned_ids(russians, EntityCategory::Unit), [7]);
        }
        let airborne: Vec<u64> = restored.airborne().map(GameEntity::stable_id).collect();
        assert_eq!(
            airborne,
            [4, 2],
            "tracked entities come back in enter order"
        );
        assert_eq!(
            crate::util::save_codec::serialize(&restored).unwrap(),
            saved
        );
    }

    #[test]
    fn building_registry_tracks_class_replacement_removal_clone_and_restore() {
        let mut store = EntityStore::new();
        let mut building = make_entity(7);
        building.set_category_for_test(EntityCategory::Structure);
        store.insert(building);
        store.insert(make_entity(2));
        let mut earlier = make_entity(3);
        earlier.set_category_for_test(EntityCategory::Structure);
        store.insert(earlier);
        store
            .get_mut(3)
            .unwrap()
            .lifecycle
            .set_object_alive_for_test(false);
        assert_eq!(
            store.cols.building_registry,
            [3, 7],
            "UnInit does not remove a class row"
        );
        store.insert(make_entity(3));
        assert_eq!(store.cols.building_registry, [7]);
        let mut replacement = make_entity(2);
        replacement.set_category_for_test(EntityCategory::Structure);
        store.insert(replacement);
        assert_eq!(store.building_registry_at(0), Some(2));
        store.remove(2);
        assert_eq!(store.building_registry_at(0), Some(7));
        assert_eq!(store.building_registry_at(1), None);
        let restored: EntityStore = crate::util::save_codec::deserialize(
            &crate::util::save_codec::serialize(&store).unwrap(),
        )
        .unwrap();
        assert_eq!(
            restored.cols.building_registry,
            store.cols.building_registry
        );
        assert_eq!(
            store.clone().cols.building_registry,
            store.cols.building_registry
        );
    }

    #[test]
    fn infantry_registry_retains_uninit_compacts_and_rebuilds_after_restore() {
        use crate::map::entities::EntityCategory;
        let mut store = EntityStore::new();
        for id in [1, 2, 3] {
            let mut e = make_entity(id);
            e.set_category_for_test(EntityCategory::Infantry);
            store.insert(e);
        }
        store
            .get_mut(1)
            .unwrap()
            .lifecycle
            .set_object_alive_for_test(false);
        store
            .get_mut(1)
            .unwrap()
            .lifecycle
            .set_in_limbo_for_test(true);
        assert_eq!(store.infantry_registry_at(0), Some(1));
        store.remove(1);
        // A caller that just visited index0 next reads index1, skipping2.
        assert_eq!(store.infantry_registry_at(1), Some(3));
        let mut appended = make_entity(4);
        appended.set_category_for_test(EntityCategory::Infantry);
        store.insert(appended);
        assert_eq!(store.infantry_registry_at(2), Some(4));
        // Replacement through the store updates category membership too.
        store.insert(make_entity(3));
        assert_eq!(store.cols.infantry_registry, [2, 4]);
        let restored: EntityStore =
            serde_json::from_str(&serde_json::to_string(&store).unwrap()).unwrap();
        assert_eq!(restored.cols.infantry_registry, [2, 4]);
        assert_eq!(restored.infantry_registry_at(2), None);
        assert_eq!(store.cols.foot_registry, [2, 3, 4]);
        assert_eq!(restored.cols.foot_registry, store.cols.foot_registry);
        let mut building = make_entity(3);
        building.set_category_for_test(EntityCategory::Structure);
        store.insert(building);
        assert_eq!(store.cols.foot_registry, [2, 4]);
    }

    /// An Anim's expiry reaches each stored entity holding the Anim, once
    /// and ascending, whether a held-Anim write stored it, the entity was
    /// stored holding it or a load restored it, and no removed holder.
    #[test]
    fn anim_holders_reach_every_holder_through_writes_inserts_and_loads() {
        let mut source = EntityStore::new();
        source.insert(make_entity(3));
        source.hold_parachute_animation(3, 50).unwrap();
        let mut store = EntityStore::new();
        store.insert(source.remove(3).unwrap());
        for id in [1, 2, 4] {
            store.insert(make_entity(id));
        }
        for id in [1, 4] {
            store.hold_parachute_animation(id, 50).unwrap();
        }
        // A missing effect slot leaves the requested Anim without a holder.
        assert_eq!(store.hold_attached_effect_anim(1, 0, 51), None);
        assert!(store.take_anim_holders(51).is_empty());
        store.remove(4);
        let loaded: EntityStore = crate::util::save_codec::deserialize(
            &crate::util::save_codec::serialize(&store).unwrap(),
        )
        .unwrap();
        for mut copy in [store.clone(), loaded] {
            assert_eq!(copy.take_anim_holders(50), [1, 3]);
        }
    }

    /// A focused id answers as the id index does while its slot moves under
    /// it (an id stored below it, a compaction), when it is removed and
    /// stored again, and when the store never held it; a load answers for
    /// its own ids.
    #[test]
    fn a_focused_id_answers_as_the_id_index_does_while_its_slot_moves() {
        let mut store = EntityStore::new();
        for id in (10..=400).step_by(2) {
            store.insert(make_entity(id));
        }
        let held = |store: &mut EntityStore, id: u64| {
            let found = [
                store.get(id).map(GameEntity::stable_id),
                store.get_mut(id).map(|entity| entity.stable_id()),
                store.get_in_slot(id).map(|(_, entity)| entity.stable_id()),
            ];
            assert_eq!(found, [store.contains(id).then_some(id); 3]);
            assert_eq!(store.owner_of(id).is_some(), store.contains(id));
            assert_eq!(store.category_of(id).is_some(), store.contains(id));
            store.contains(id)
        };
        store.focus(300);
        assert!(held(&mut store, 300));
        // An id stored below it shifts its slot by one.
        store.insert(make_entity(11));
        assert!(held(&mut store, 300));
        // The removal that compacts the store moves it again.
        let slots = store.cols.ids.len();
        let mut below = (10..200).step_by(2);
        while store.cols.ids.len() == slots {
            store.remove(below.next().expect("a removal compacts the store"));
        }
        assert!(held(&mut store, 300));
        store.remove(300);
        assert!(!held(&mut store, 300));
        store.insert(make_entity(300));
        assert!(held(&mut store, 300));
        // An id the store never held is absent until it is stored.
        store.focus(7);
        assert!(!held(&mut store, 7));
        store.insert(make_entity(7));
        assert!(held(&mut store, 7) && held(&mut store, 300) && !held(&mut store, 10));
        let mut loaded: EntityStore = crate::util::save_codec::deserialize(
            &crate::util::save_codec::serialize(&store).unwrap(),
        )
        .unwrap();
        let mut copy = store.clone();
        for other in [&mut loaded, &mut copy] {
            assert!(held(other, 7) && held(other, 300) && !held(other, 0));
        }
    }

    #[test]
    fn clear_radio_contacts_removes_reciprocal_contacts() {
        let mut store = EntityStore::new();
        let mut first = make_entity(1);
        let mut second = make_entity(2);
        first.mark_live_contact_with(2);
        second.mark_live_contact_with(1);
        store.insert(first);
        store.insert(second);

        store.clear_radio_contacts_for(1);

        assert!(store.get(1).unwrap().seldom.radio_contacts.is_empty());
        assert!(store.get(2).unwrap().seldom.radio_contacts.is_empty());
    }

    #[test]
    fn clear_radio_contacts_preserves_remaining_order() {
        let mut store = EntityStore::new();
        let mut entity = make_entity(1);
        entity.seldom.radio_contacts.set_capacity(4); // hold more than one contact
        entity.mark_live_contact_with(2);
        entity.mark_live_contact_with(3);
        entity.mark_live_contact_with(4);
        store.insert(entity);
        store.insert(make_entity(3));

        store.clear_radio_contacts_for(3);

        // Removal nulls slot 1 in place (no compaction): 2 and 4 keep their slots.
        let contacts = &store.get(1).unwrap().seldom.radio_contacts;
        assert_eq!(contacts.len(), 2);
        assert!(contacts.contains(2) && contacts.contains(4) && !contacts.contains(3));
        assert_eq!(contacts.find_slot(2), Some(0));
        assert_eq!(contacts.find_slot(4), Some(2));
    }

    /// The store's saved form.
    fn save(store: &EntityStore) -> Vec<u8> {
        crate::util::save_codec::serialize(store).unwrap()
    }

    /// A store of `ids`, each a tank standing at its own x.
    fn store_of(ids: impl IntoIterator<Item = u64>) -> EntityStore {
        let mut store = EntityStore::new();
        for id in ids {
            let mut entity = make_entity(id);
            entity.position.set_rx_for_test(id as u16);
            store.insert(entity);
        }
        store
    }

    /// What `store` reads by id for each of `ids`: the entity's slot and
    /// saved form, and its sleep cell's marks.
    fn reads(store: &EntityStore, ids: &[u64]) -> Vec<Option<(usize, String, [u32; 5])>> {
        ids.iter()
            .map(|&id| {
                let (slot, entity) = store.get_in_slot(id)?;
                assert!(std::ptr::eq(store.get(id)?, entity));
                let cell = store.cell(id)?;
                let marks = [
                    cell.wake_at,
                    cell.owed,
                    cell.note.into(),
                    cell.fit_until,
                    cell.marks.into(),
                ];
                Some((slot, serde_json::to_string(entity).unwrap(), marks))
            })
            .collect()
    }

    /// What `store`'s walks read: each entity's id, x and AI counter, by
    /// every walk, and the sleep cells' tallies.
    fn walks(store: &EntityStore) -> Vec<String> {
        let fact = |entity: &GameEntity| {
            format!(
                "{}:{}:{}",
                entity.stable_id(),
                entity.position.rx(),
                entity.mission.ai_counter()
            )
        };
        vec![
            store.values().map(fact).collect::<Vec<_>>().join(" "),
            store
                .iter_sorted()
                .rev()
                .map(|(_, entity)| fact(entity))
                .collect::<Vec<_>>()
                .join(" "),
            store
                .values_of_category(EntityCategory::Unit)
                .map(fact)
                .collect::<Vec<_>>()
                .join(" "),
            store
                .values_of_owners(|_, _| true)
                .map(fact)
                .collect::<Vec<_>>()
                .join(" "),
            store.par_map(|_, entity| fact(entity)).join(" "),
            store
                .par_filter_map(|_, entity| Some(fact(entity)))
                .join(" "),
            format!(
                "{:?} {:?} {}",
                store.par_select(|_, entity| entity.position.rx() % 2 == 0),
                store.owed_in_value_order(),
                store.sleeping_count()
            ),
        ]
    }

    /// A reader's take of each touch log and of the look log, as sets.
    fn logged(store: &mut EntityStore) -> [BTreeSet<u64>; 3] {
        let set = |touched| match touched {
            Touched::Ids(ids) => ids.into_iter().collect(),
            Touched::All => panic!("a log of ids"),
        };
        [
            set(store.take_touched(TouchReader::BlockerPlane)),
            set(store.take_touched(TouchReader::ExpiryListeners)),
            set(store.take_looks()),
        ]
    }

    /// A fork reads as an ordinary copy making the same writes does, through
    /// its focused entry and the others alike, while its base and a sibling
    /// fork read as before; committed to the unshared base, its writes leave
    /// the base as that copy: the same saved form, reads and touch and look
    /// logs, a log narrowed to buildings included.
    #[test]
    fn a_fork_reads_and_commits_as_a_copy_making_its_writes() {
        let ids = [2, 3, 5, 8, 13, 21, 34];
        let mut base = store_of([2, 3, 5, 8, 13, 21]);
        base.insert(GameEntity::test_default_of_category(
            34,
            "GAPOWR",
            "Americans",
            10,
            10,
            EntityCategory::Structure,
        ));
        base.sleep_until(5, 50, 1);
        base.owe_visit(5);
        base.owe_visit(5);
        base.set_quiet(8);
        let mut copy = base.clone();
        // A new store's logs and a clone's report everything: empty them.
        for store in [&mut base, &mut copy] {
            store.take_touched(TouchReader::BlockerPlane);
            store.take_touched(TouchReader::ExpiryListeners);
            store.take_looks();
            store.narrow_touched(TouchReader::BlockerPlane, true);
        }
        let before = (reads(&base, &ids), walks(&base));
        base.share();
        let mut fork = base.fork();
        let sibling = base.fork();
        fn worker_safe<T: Send + Sync>(_: &T) {}
        worker_safe(&fork);
        let mut seed = 0x9e37_79b9_7f4a_7c15_u64;
        let mut next = |bound: usize| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed % bound as u64) as usize
        };
        for step in 0..400_u32 {
            let id = ids[next(ids.len())];
            let op = next(11);
            let x = step as u16;
            for store in [&mut fork, &mut copy] {
                match op {
                    0 => store.focus(id),
                    1 => {
                        if let Some(entity) = store.get_mut(id) {
                            entity.position.set_rx_for_test(x);
                        }
                    }
                    2 => {
                        if let Some(entity) =
                            store.get_mut_if(id, |entity| entity.position.rx() % 2 == 0)
                        {
                            entity.position.set_rx_for_test(x + 1);
                        }
                    }
                    3 => store.for_each_entry_mut(
                        |_| true,
                        |entry| {
                            if entry.id() == id && entry.get().position.rx() != x {
                                entry.into_mut().position.set_rx_for_test(x);
                            }
                        },
                    ),
                    4 => store.count_mission_ai_visit(id),
                    5 => {
                        if let Some(entity) = store.unlogged_mut(id) {
                            entity.position.set_rx_for_test(x + 2);
                        }
                    }
                    6 => store.sleep_until(id, 60 + step, 2),
                    7 => {
                        store.owe_visit(id);
                        store.fit_to_sleep(id, step % 3);
                    }
                    8 => store.set_quiet(id),
                    9 => store.wake(id),
                    _ => store.note_look(id),
                }
            }
            assert_eq!(reads(&fork, &ids), reads(&copy, &ids), "step {step}");
        }
        assert_eq!(walks(&fork), walks(&copy));
        assert_eq!(
            (reads(&base, &ids), walks(&base)),
            before,
            "the base reads as before"
        );
        assert_eq!(
            (reads(&sibling, &ids), walks(&sibling)),
            before,
            "and so does a sibling fork"
        );
        let writes = fork.take_fork_writes();
        assert!(!writes.structural());
        drop((fork, sibling));
        base.unshare();
        base.commit_writes(writes);
        assert_eq!(save(&base), save(&copy));
        assert_eq!(
            (reads(&base, &ids), walks(&base)),
            (reads(&copy, &ids), walks(&copy))
        );
        let logs = logged(&mut copy);
        assert_eq!(logs[0], BTreeSet::from([34]), "the narrowed log");
        assert!(logs[1].len() > 1 && !logs[2].is_empty());
        assert_eq!(logged(&mut base), logs);
    }

    /// A fork's hand-out of a sleeping object wakes it and settles the visits
    /// it is owed in the fork's copy alone, and one of a quiet and steady
    /// object clears both marks there alone; a reset returns the fork to its
    /// base, ready for the next turn.
    #[test]
    fn a_fork_wakes_and_settles_in_its_own_copy_until_reset() {
        let mut base = store_of([1, 2]);
        base.sleep_until(1, 90, 3);
        base.owe_visit(1);
        base.owe_visit(1);
        base.set_quiet(2);
        base.set_steady(2);
        base.share();
        let marks = |store: &EntityStore| {
            (
                store.sleeps_until(1),
                store.owed_visits(1),
                store.get(1).unwrap().mission.ai_counter(),
                (store.is_quiet(2), store.is_steady(2)),
            )
        };
        let asleep = marks(&base);
        assert_eq!(
            (asleep.0, asleep.1, asleep.3),
            (Some((90, 3)), 2, (true, true))
        );
        let mut fork = base.fork();
        fork.focus(1);
        fork.get_mut(1);
        fork.get_mut(2);
        assert_eq!(marks(&fork), (None, 0, asleep.2 + 2, (false, false)));
        assert_eq!(marks(&base), asleep);
        fork.reset_fork();
        assert_eq!(marks(&fork), asleep);
        assert_eq!(fork.take_fork_writes().ids().count(), 0);
        fork.get_mut(2);
        assert_eq!(fork.take_fork_writes().ids().collect::<Vec<_>>(), [2]);
    }

    /// A structural write gives the fork a store of its own: it reads what it
    /// wrote before and after, through every index, and its writes cannot be
    /// merged; the base never sees them, and a take returns the fork there.
    #[test]
    fn a_structural_write_keeps_the_fork_a_store_of_its_own() {
        let mut base = store_of([1, 2, 3]);
        let saved = save(&base);
        let russians = crate::sim::intern::test_intern("Russians");
        base.share();
        let mut fork = base.fork();
        fork.focus(2);
        fork.get_mut(2).unwrap().position.set_rx_for_test(77);
        fork.insert(make_entity(9));
        fork.remove(1);
        fork.change_owner(3, russians);
        fork.get_mut(9).unwrap().position.set_rx_for_test(78);
        assert_eq!(fork.keys_sorted(), [2, 3, 9]);
        assert_eq!(
            [2, 9].map(|id| fork.get(id).unwrap().position.rx()),
            [77, 78]
        );
        assert_eq!(fork.owned_ids(russians, EntityCategory::Unit), [3]);
        assert_eq!(fork.cols.techno_registry, [2, 3, 9]);
        let writes = fork.take_fork_writes();
        assert!(writes.structural() && writes.ids().next().is_none());
        assert_eq!(save(&fork), saved, "a taken fork reads its base");
        drop(fork);
        assert_eq!(save(&base), saved);
        base.unshare();
        assert_eq!(save(&base), saved);
    }

    /// Only a store no fork reads can own its columns again.
    #[test]
    #[should_panic(expected = "fork still alive")]
    fn unsharing_with_a_live_fork_panics() {
        let mut store = store_of([1]);
        store.share();
        let _fork = store.fork();
        store.unshare();
    }

    /// A shared store is the frozen base its forks read.
    #[test]
    #[should_panic(expected = "write to a shared entity store")]
    fn a_write_to_a_shared_store_panics() {
        let mut store = store_of([1]);
        store.share();
        store.get_mut(1);
    }

    /// `written_this_pass` is false before a pass's first write of an entity
    /// and true after a hand-out, a sleep-mark write or a commit of it, until
    /// the next pass begins; a clone or a load starts unstamped.
    #[test]
    fn written_this_pass_follows_hand_outs_and_commits() {
        let mut store = store_of([1, 2, 3]);
        store.get_mut(1);
        store.begin_pass();
        assert!(!store.written_this_pass(1), "written before the pass");
        store.get_mut(1);
        store.set_quiet(3);
        assert_eq!(
            [1, 2, 3].map(|id| store.written_this_pass(id)),
            [true, false, true]
        );
        store.share();
        let [first, second] = [store.fork(), store.fork()].map(|mut fork| {
            fork.get_mut(2);
            fork.take_fork_writes()
        });
        store.unshare();
        assert!(!store.written_this_pass(2));
        store.commit_writes(first);
        assert!(store.written_this_pass(2), "a commit stamps what it writes");
        assert!(
            second.ids().any(|id| store.written_this_pass(id)),
            "so the second turn's write of 2 meets a stamp"
        );
        let loaded: EntityStore = crate::util::save_codec::deserialize(&save(&store)).unwrap();
        for copy in [store.clone(), loaded] {
            assert!(!copy.written_this_pass(1), "a copy starts unstamped");
        }
        store.begin_pass();
        assert_eq!([1, 2, 3].map(|id| store.written_this_pass(id)), [false; 3]);
    }

    /// Sharing moves the columns without changing what the store holds: its
    /// saved form, and a fork's, are the ordinary store's before and after.
    #[test]
    fn a_share_and_unshare_leave_the_saved_form_as_it_was() {
        let mut store = store_of([4, 9, 2]);
        store.sleep_until(9, 30, 0);
        store.owe_visit(9);
        let saved = save(&store);
        store.share();
        assert_eq!(save(&store), saved);
        assert_eq!(save(&store.fork()), saved);
        store.unshare();
        assert_eq!(save(&store), saved);
    }
}
