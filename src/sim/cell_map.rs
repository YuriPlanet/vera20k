//! A map keyed by native cell coordinates, dense over gamemd's cell array.
//!
//! ## Forks (VERA-only, sim-scale)
//! For object turns run side by side, as the entity store's are (see its
//! module documentation): [`CellMap::share`] moves the cells, uncopied,
//! behind one `Arc`, and the shared map answers reads and refuses writes.
//! [`CellMap::fork`] makes a map that reads them and keeps its own writes by
//! key, in the order first written: a cell's value is copied from the base
//! on its first write, and later reads see the copy.
//! [`CellMap::take_fork_writes`] takes them, which returns the fork to its
//! base in time proportional to what it wrote, and [`CellMap::commit_writes`]
//! applies them to the map once [`CellMap::unshare`]d, through the inserts
//! and removals a direct write makes: a key beyond the dense array grows it,
//! or lands among the keys outside it, as it would have. Only a walk handing
//! out every value (a load's) and a write its grid keeps beside the map
//! ([`CellMap::note_unmergeable_write`]) cannot be merged
//! ([`CellWrites::structural`]). [`CellMap::written_this_pass`] tells which
//! cells were written since [`CellMap::begin_pass`].

use std::collections::BTreeMap;
use std::fmt;
use std::marker::PhantomData;
use std::sync::Arc;

use serde::de::{Deserialize, Deserializer, MapAccess, Visitor};
use serde::ser::{Serialize, SerializeMap, Serializer};

/// Cells per side of gamemd's fixed cell array, the dense side a map starts with.
const RETAIL_SIDE: usize = crate::map::cell_index::CELL_ROW_STRIDE as usize;
/// Cells per side of the largest cell array a map can select.
const SIDE: usize = crate::map::cell_index::MAX_CELL_SIDE as usize;
// Both sides are powers of two, so a slot splits into its strip and offset,
// or into its key, by a shift and a mask. The side is a field the compiler
// cannot see through, so `/` and `%` by it would divide on every lookup.
const _: () = assert!(RETAIL_SIDE.is_power_of_two() && SIDE.is_power_of_two());

/// What a write to a shared map panics with.
const SHARED_WRITE: &str = "write to a shared cell map: while shared it is the frozen base its forks read; write through a fork, or unshare it first";

/// A `BTreeMap<(u16, u16), V>` with O(1) lookups: each native cell has a
/// dense slot.
///
/// It keeps the sorted map's observable behavior: iteration visits keys in
/// ascending `(x, y)` order, and equality, `Debug` and serde see the same
/// entries (serde writes the same map, so saves stay byte-identical). Slots
/// are x-major (`x * 512 + y`), so key order is storage order, and a bitset
/// of filled slots lets iteration skip empty runs. Storage comes in strips of
/// one column, allocated on their first insert, so a small map pays for the
/// columns it uses. The dense side is the retail 512 until a key of an
/// extended map's larger array arrives, which relays it out once at 1024, so
/// retail maps keep their footprint. Keys outside the largest array, which no
/// real cell produces, keep a sorted map of their own.
pub(crate) struct CellMap<V> {
    /// The cells while the map owns them: an ordinary map's, or a fork's own
    /// copy after a walk writing every value ([`Self::materialise`]). Empty
    /// while the map reads through `view`.
    own: Cells<V>,
    /// VERA-only (sim-scale): the shared cells a shared map or a fork reads,
    /// with a fork's writes. `None` for an ordinary map, whose every access
    /// takes one predictable branch on it.
    view: Option<View<V>>,
    /// A fork that met a walk writing every value: its view, set aside while
    /// it reads its own copy, for [`Self::reset_fork`].
    set_aside: Option<View<V>>,
    mark: crate::util::synced::SyncMark,
}

/// A map's cells, and what changed in them since the presentation's last
/// sync and since the pass began: what [`CellMap::share`] puts behind one
/// `Arc`.
#[derive(Clone)]
struct Cells<V> {
    /// Dense cells per side: [`RETAIL_SIDE`], or [`SIDE`] once grown.
    side: usize,
    /// One column each; the presentation copy takes only the cells written
    /// since its last sync ([`CellMap::sync_into`], docs/sim-thread.md).
    strips: Vec<Option<Box<[Option<V>]>>>,
    /// One bit per slot, set while the slot holds a value.
    filled: Vec<u64>,
    filled_count: usize,
    outside: BTreeMap<(u16, u16), V>,
    /// One bit per dense slot written since the last sync, so a sync that a
    /// moving crowd dirties across most strips still copies only the cells
    /// that changed. Empty until the dense storage is allocated.
    changed_cells: Vec<u64>,
    /// The layout or `outside` changed since the last sync.
    reshaped: bool,
    stamps: Stamps,
}

/// VERA-only (sim-scale): one bit per dense slot whose value a write reached
/// since [`CellMap::begin_pass`] (a hand-out for writing, an insert or a
/// removal, a commit), laid out as `filled`: a pass clears 32 KB at the
/// retail side. Empty before the first pass, when writes stamp nothing, and
/// in a copy. Never saved or hashed.
#[derive(Default)]
struct Stamps(Vec<u64>);

impl Clone for Stamps {
    /// A copy is not in a pass.
    fn clone(&self) -> Self {
        Self::default()
    }
}

impl Stamps {
    /// Stamp `slot`; nothing before the first pass.
    #[inline]
    fn set(&mut self, slot: usize) {
        if let Some(word) = self.0.get_mut(slot / 64) {
            *word |= 1 << (slot % 64);
        }
    }
}

/// What a shared map or a fork reads through.
struct View<V> {
    /// The shared cells, frozen while any fork of them lives.
    base: Arc<Cells<V>>,
    /// A fork's view, which keeps its writes in `overlay`; the shared map
    /// itself refuses writes.
    fork: bool,
    /// The fork's writes since its last take or reset: each cell's value as
    /// the fork holds it (`None`: empty), in the order first written. A turn
    /// writes a few cells, so a linear search serves.
    overlay: Vec<((u16, u16), Option<V>)>,
    /// The fork wrote what its grid keeps beside the map
    /// ([`CellMap::note_unmergeable_write`]).
    unmergeable: bool,
}

/// One object's turn's writes in a fork ([`CellMap::take_fork_writes`]),
/// for [`CellMap::commit_writes`] on the map the fork's base was shared from.
#[derive(Debug)]
pub(crate) struct CellWrites<V> {
    /// Each cell the turn wrote and its value after the turn (`None`:
    /// empty), in the order first written.
    cells: Vec<((u16, u16), Option<V>)>,
    /// The turn met a write a commit cannot carry; nothing else is kept.
    structural: bool,
}

impl<V> CellWrites<V> {
    /// The turn met a write a commit cannot carry: a walk writing every
    /// value, or a write of what its grid keeps beside the map. Its writes
    /// cannot be merged, and it runs again on the map itself.
    pub(crate) fn structural(&self) -> bool {
        self.structural
    }

    /// The cells the turn wrote, each once, in the order first written:
    /// those it left as it found them too.
    pub(crate) fn cells(&self) -> impl Iterator<Item = (u16, u16)> + '_ {
        self.cells.iter().map(|&(key, _)| key)
    }
}

impl<V> Default for CellMap<V> {
    fn default() -> Self {
        Self {
            own: Cells::default(),
            view: None,
            set_aside: None,
            mark: crate::util::synced::SyncMark::new(),
        }
    }
}

impl<V> Default for Cells<V> {
    fn default() -> Self {
        Self {
            side: RETAIL_SIDE,
            strips: Vec::new(),
            filled: Vec::new(),
            filled_count: 0,
            outside: BTreeMap::new(),
            changed_cells: Vec::new(),
            reshaped: true,
            stamps: Stamps::default(),
        }
    }
}

/// A clone is an ordinary map holding what this one reads, a fork's writes
/// included, with no cell stamped.
impl<V: Clone> Clone for CellMap<V> {
    fn clone(&self) -> Self {
        Self {
            own: match &self.view {
                None => self.own.clone(),
                Some(view) => view.copy(),
            },
            view: None,
            set_aside: None,
            mark: self.mark.clone(),
        }
    }
}

/// Mark `slot` written since the last sync, in a dense map of `side` slots per
/// strip.
#[inline]
fn note_slot(changed_cells: &mut Vec<u64>, side: usize, slot: usize) {
    let words = side * side / 64;
    if changed_cells.len() < words {
        changed_cells.resize(words, 0);
    }
    changed_cells[slot / 64] |= 1 << (slot % 64);
}

/// A cell value the presentation copy can bring up to date in place.
///
/// [`CellMap::sync_into`] copies only the cells a write reached; for a value
/// that owns a `Vec` (the occupancy lists) an ordinary `clone_from` would drop
/// and reallocate the destination's storage on every sync. This lets the value
/// reuse it instead.
pub(crate) trait CellValue: Clone {
    fn copy_cell_from(&mut self, source: &Self);
}

impl CellValue for u32 {
    #[inline]
    fn copy_cell_from(&mut self, source: &Self) {
        *self = *source;
    }
}

impl CellValue for u64 {
    #[inline]
    fn copy_cell_from(&mut self, source: &Self) {
        *self = *source;
    }
}

/// Whether `key` lies in the largest dense array rather than `outside`.
fn is_dense((x, y): (u16, u16)) -> bool {
    usize::from(x) < SIDE && usize::from(y) < SIDE
}

/// Merge two key-ordered entry streams into one.
fn merge_by_key<T>(
    inside: impl Iterator<Item = ((u16, u16), T)>,
    outside: impl Iterator<Item = ((u16, u16), T)>,
) -> impl Iterator<Item = ((u16, u16), T)> {
    let mut inside = inside.peekable();
    let mut outside = outside.peekable();
    std::iter::from_fn(move || match (inside.peek(), outside.peek()) {
        (Some((a, _)), Some((b, _))) if b < a => outside.next(),
        (Some(_), _) => inside.next(),
        (None, _) => outside.next(),
    })
}

/// An ordinary map's entries, or a fork's merged with its writes.
enum Entries<A, B> {
    Own(A),
    Merged(B),
}

impl<T, A: Iterator<Item = T>, B: Iterator<Item = T>> Iterator for Entries<A, B> {
    type Item = T;

    #[inline]
    fn next(&mut self) -> Option<T> {
        match self {
            Self::Own(entries) => entries.next(),
            Self::Merged(entries) => entries.next(),
        }
    }
}

impl<V: Clone> CellMap<V> {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn len(&self) -> usize {
        match &self.view {
            None => self.own.len(),
            Some(view) => view.len(),
        }
    }

    /// Whether the map holds its cells itself: neither shared nor a fork.
    fn owns_cells(&self) -> bool {
        self.view.is_none() && self.set_aside.is_none()
    }
}

impl<V: Clone + CellValue> CellMap<V> {
    /// Bring `mirror`, the presentation copy, up to this map: the cells
    /// written since the last sync into it, or all of it when it is another
    /// copy or the layout changed.
    pub(crate) fn sync_into(&mut self, mirror: &mut Self) {
        assert!(
            self.owns_cells() && mirror.owns_cells(),
            "sync of a shared or forked cell map"
        );
        let (cells, copy) = (&mut self.own, &mut mirror.own);
        let full = cells.reshaped || !self.mark.knows(&mirror.mark);
        if full {
            copy.side = cells.side;
            copy.strips.clone_from(&cells.strips);
            copy.outside.clone_from(&cells.outside);
        } else {
            let side = cells.side;
            for (word, bits) in cells.changed_cells.iter().enumerate() {
                let mut bits = *bits;
                while bits != 0 {
                    let slot = word * 64 + bits.trailing_zeros() as usize;
                    bits &= bits - 1;
                    let strip = slot / side;
                    let offset = slot % side;
                    let Some(source) = cells
                        .strips
                        .get(strip)
                        .and_then(|strip| strip.as_deref())
                        .map(|strip| &strip[offset])
                    else {
                        continue;
                    };
                    let target = copy.strips[strip]
                        .get_or_insert_with(|| vec![None; side].into_boxed_slice());
                    match (&mut target[offset], source) {
                        (Some(target), Some(source)) => target.copy_cell_from(source),
                        (target, source) => *target = source.clone(),
                    }
                }
            }
        }
        copy.filled.clone_from(&cells.filled);
        copy.filled_count = cells.filled_count;
        cells.changed_cells.iter_mut().for_each(|word| *word = 0);
        cells.reshaped = false;
        self.mark.synced(&mirror.mark);
    }
}

impl<V: Clone> CellMap<V> {
    #[inline]
    pub(crate) fn get(&self, key: &(u16, u16)) -> Option<&V> {
        match &self.view {
            None => self.own.get(key),
            Some(view) => view.get(key),
        }
    }

    #[inline]
    pub(crate) fn get_mut(&mut self, key: &(u16, u16)) -> Option<&mut V> {
        match &mut self.view {
            None => self.own.get_mut(key),
            Some(view) => view.get_mut(*key),
        }
    }

    /// The value at `key`, inserting `make()` first when the cell is empty.
    #[inline]
    pub(crate) fn get_or_insert_with(
        &mut self,
        key: (u16, u16),
        make: impl FnOnce() -> V,
    ) -> &mut V {
        match &mut self.view {
            None => self.own.get_or_insert_with(key, make),
            Some(view) => view.get_or_insert_with(key, make),
        }
    }

    pub(crate) fn insert(&mut self, key: (u16, u16), value: V) -> Option<V> {
        let previous = self.remove(&key);
        self.get_or_insert_with(key, || value);
        previous
    }

    #[inline]
    pub(crate) fn remove(&mut self, key: &(u16, u16)) -> Option<V> {
        match &mut self.view {
            None => self.own.remove(key),
            Some(view) => view.remove(*key),
        }
    }

    /// Entries in ascending `(x, y)` order.
    pub(crate) fn iter(&self) -> impl Iterator<Item = ((u16, u16), &V)> + '_ {
        match &self.view {
            None => Entries::Own(self.own.iter()),
            Some(view) if view.overlay.is_empty() => Entries::Own(view.base.iter()),
            Some(view) => Entries::Merged(view.iter()),
        }
    }

    /// Mutable entries in ascending `(x, y)` order. A fork takes its own
    /// copy first, and its writes cannot be merged.
    fn iter_mut(&mut self) -> impl Iterator<Item = ((u16, u16), &mut V)> + '_ {
        if self.view.is_some() {
            self.materialise();
        }
        self.own.iter_mut()
    }

    pub(crate) fn values(&self) -> impl Iterator<Item = &V> + '_ {
        self.iter().map(|(_, value)| value)
    }

    pub(crate) fn values_mut(&mut self) -> impl Iterator<Item = &mut V> + '_ {
        self.iter_mut().map(|(_, value)| value)
    }

    /// Before a write of what the map's grid keeps beside it, which a commit
    /// cannot carry: a fork's writes become unmergeable and a shared map
    /// refuses; an ordinary map carries on.
    #[inline]
    pub(crate) fn note_unmergeable_write(&mut self) {
        if let Some(view) = &mut self.view {
            assert!(view.fork, "{SHARED_WRITE}");
            view.unmergeable = true;
        }
    }

    /// Give a fork its own copy of its base, its writes applied, and set its
    /// view aside: from here to [`Self::reset_fork`] it is an ordinary map
    /// whose writes cannot be merged ([`CellWrites::structural`]). A shared
    /// map refuses.
    #[cold]
    #[inline(never)]
    fn materialise(&mut self) {
        let Some(mut view) = self.view.take() else {
            return;
        };
        if !view.fork {
            self.view = Some(view);
            panic!("{SHARED_WRITE}");
        }
        self.own = view.copy();
        view.overlay.clear();
        self.set_aside = Some(view);
    }
}

/// Forks (VERA-only, sim-scale; see the module documentation), through the
/// grids that hold a map.
impl<V: Clone> CellMap<V> {
    /// Make this map the frozen base its [`Self::fork`]s read: the cells move
    /// behind one `Arc`, uncopied, and the map answers every read as before
    /// and refuses writes until [`Self::unshare`].
    pub(crate) fn share(&mut self) {
        assert!(
            self.owns_cells(),
            "share of a cell map that is shared or a fork"
        );
        self.view = Some(View {
            base: Arc::new(std::mem::take(&mut self.own)),
            fork: false,
            overlay: Vec::new(),
            unmergeable: false,
        });
    }

    /// Own the cells again after [`Self::share`]. Panics while a fork of them
    /// is alive.
    pub(crate) fn unshare(&mut self) {
        assert!(
            self.view.as_ref().is_some_and(|view| !view.fork),
            "unshare of a cell map that is not shared"
        );
        let view = self.view.take().expect("checked above");
        self.own = Arc::try_unwrap(view.base).unwrap_or_else(|_| {
            panic!("unshare of a cell map with a fork still alive: drop every fork first")
        });
    }

    /// A map that reads this shared one ([`Self::share`]) and keeps its own
    /// writes, for one object's turn at a time ([`Self::take_fork_writes`],
    /// [`Self::reset_fork`]). Making one copies no cell.
    pub(crate) fn fork(&self) -> Self {
        let view = self
            .view
            .as_ref()
            .filter(|view| !view.fork)
            .expect("fork of a cell map that is not shared");
        Self {
            own: Cells::default(),
            view: Some(View {
                base: Arc::clone(&view.base),
                fork: true,
                overlay: Vec::new(),
                unmergeable: false,
            }),
            set_aside: None,
            mark: crate::util::synced::SyncMark::new(),
        }
    }

    /// What this fork wrote since its last take or reset, leaving it as
    /// [`Self::reset_fork`] does.
    pub(crate) fn take_fork_writes(&mut self) -> CellWrites<V> {
        let writes = match (&mut self.view, &self.set_aside) {
            (Some(view), _) if view.fork => CellWrites {
                cells: if view.unmergeable {
                    Vec::new()
                } else {
                    std::mem::take(&mut view.overlay)
                },
                structural: view.unmergeable,
            },
            (None, Some(_)) => CellWrites {
                cells: Vec::new(),
                structural: true,
            },
            _ => panic!("take_fork_writes of a cell map that is not a fork"),
        };
        self.reset_fork();
        writes
    }

    /// Drop what this fork wrote, and its own copy if it made one: it reads
    /// its base again. Costs what the fork wrote.
    pub(crate) fn reset_fork(&mut self) {
        if let Some(view) = self.set_aside.take() {
            self.own = Cells::default();
            self.view = Some(view);
        }
        let view = self
            .view
            .as_mut()
            .filter(|view| view.fork)
            .expect("reset_fork of a cell map that is not a fork");
        view.overlay.clear();
        view.unmergeable = false;
    }

    /// Apply one turn's writes from a fork ([`Self::take_fork_writes`]) to
    /// this map, which shared the fork's base and has been unshared since:
    /// in the order first written, each cell takes the fork's value through
    /// the insert or the removal a direct write makes, and is stamped for
    /// [`Self::written_this_pass`]. `emptied` receives the value of each
    /// cell the commit empties. Structural writes are refused: the caller
    /// runs such a turn again on this map.
    pub(crate) fn commit_writes(&mut self, writes: CellWrites<V>, mut emptied: impl FnMut(V)) {
        assert!(
            self.owns_cells(),
            "commit_writes to a cell map that is shared or a fork"
        );
        debug_assert!(
            !writes.structural,
            "a structural turn's writes cannot be merged: run the turn again"
        );
        if writes.structural {
            return;
        }
        for (key, value) in writes.cells {
            let empties = value.is_none();
            if let Some(old) = self.own.replace(key, value)
                && empties
            {
                emptied(old);
            }
        }
    }

    /// Begin a pass for [`Self::written_this_pass`]: from here every write
    /// that reaches a cell's value, and every commit, stamps the cell. For
    /// the map itself, before its forks' base is shared.
    pub(crate) fn begin_pass(&mut self) {
        assert!(
            self.owns_cells(),
            "begin_pass of a cell map that is shared or a fork"
        );
        let words = self.own.side * self.own.side / 64;
        let stamps = &mut self.own.stamps.0;
        stamps.clear();
        stamps.resize(words, 0);
    }

    /// Whether `key`'s cell was written since [`Self::begin_pass`]: its value
    /// handed out for writing, inserted or removed, or committed. False
    /// before the first pass and for a fork; true for a key beyond the dense
    /// array, which keeps no stamp.
    pub(crate) fn written_this_pass(&self, key: (u16, u16)) -> bool {
        match &self.view {
            None if self.set_aside.is_none() => self.own.stamped(key),
            Some(view) if !view.fork => view.base.stamped(key),
            _ => false,
        }
    }
}

impl<V: Clone> Cells<V> {
    fn len(&self) -> usize {
        self.filled_count + self.outside.len()
    }

    /// Slots per strip: one column of the dense array, so a sync copies only
    /// the columns written since the last.
    fn strip_slots(&self) -> usize {
        self.side
    }

    /// The dense slot of `key`; `None` when it lies outside the current side.
    fn slot_of(&self, (x, y): (u16, u16)) -> Option<usize> {
        let (x, y) = (usize::from(x), usize::from(y));
        (x < self.side && y < self.side).then_some(x * self.side + y)
    }

    /// log2 of the dense side.
    fn side_shift(&self) -> u32 {
        self.side.trailing_zeros()
    }

    fn key_of(&self, slot: usize) -> (u16, u16) {
        (
            (slot >> self.side_shift()) as u16,
            (slot & (self.side - 1)) as u16,
        )
    }

    /// `slot`'s strip and its offset there: the slot over, and modulo,
    /// [`Self::strip_slots`].
    fn strip_and_offset(&self, slot: usize) -> (usize, usize) {
        let shift = self.side_shift();
        (slot >> shift, slot & ((1 << shift) - 1))
    }

    fn slot(&self, slot: usize) -> Option<&Option<V>> {
        let (strip, offset) = self.strip_and_offset(slot);
        let strip = self.strips.get(strip)?.as_deref()?;
        Some(&strip[offset])
    }

    /// Relay the dense storage out at the largest side. Runs once, on the
    /// first key of an extended map's array beyond the retail side; the
    /// pass's stamps follow their cells.
    fn grow(&mut self) {
        self.reshaped = true;
        self.changed_cells.clear();
        let stamped = std::mem::take(&mut self.stamps.0);
        let old_side = std::mem::replace(&mut self.side, SIDE);
        let old_strips = std::mem::take(&mut self.strips);
        self.filled = Vec::new();
        self.filled_count = 0;
        for (strip, slots) in old_strips.into_iter().enumerate() {
            for (offset, value) in slots
                .into_iter()
                .flat_map(|slots| slots.to_vec())
                .enumerate()
            {
                if let Some(value) = value {
                    // One column per strip at the old side.
                    let slot = strip * old_side + offset;
                    let key = ((slot / old_side) as u16, (slot % old_side) as u16);
                    self.get_or_insert_with(key, || value);
                }
            }
        }
        if !stamped.is_empty() {
            self.stamps.0 = vec![0; SIDE * SIDE / 64];
            for (word, &bits) in stamped.iter().enumerate() {
                for bit in SetBits(bits) {
                    let slot = word * 64 + bit;
                    self.stamps.set((slot / old_side) * SIDE + slot % old_side);
                }
            }
        }
    }

    fn get(&self, key: &(u16, u16)) -> Option<&V> {
        match self.slot_of(*key) {
            Some(slot) => self.slot(slot)?.as_ref(),
            None if is_dense(*key) => None,
            None => self.outside.get(key),
        }
    }

    fn get_mut(&mut self, key: &(u16, u16)) -> Option<&mut V> {
        match self.slot_of(*key) {
            Some(slot) => {
                let (strip, offset) = self.strip_and_offset(slot);
                let slots = self.strips.get_mut(strip)?.as_mut()?;
                let value = slots[offset].as_mut()?;
                note_slot(&mut self.changed_cells, self.side, slot);
                self.stamps.set(slot);
                Some(value)
            }
            None if is_dense(*key) => None,
            None => self.outside.get_mut(key),
        }
    }

    fn get_or_insert_with(&mut self, key: (u16, u16), make: impl FnOnce() -> V) -> &mut V {
        if !is_dense(key) {
            self.reshaped = true;
            return self.outside.entry(key).or_insert_with(make);
        }
        if self.slot_of(key).is_none() {
            self.grow();
        }
        let slot = self.slot_of(key).expect("grown side holds every dense key");
        let strip_slots = self.strip_slots();
        let (strip, offset) = self.strip_and_offset(slot);
        if self.strips.is_empty() {
            self.strips.resize_with(self.side, || None);
            self.filled = vec![0; self.side * self.side / 64];
            self.changed_cells.clear();
            self.reshaped = true;
        }
        note_slot(&mut self.changed_cells, self.side, slot);
        self.stamps.set(slot);
        let strip =
            self.strips[strip].get_or_insert_with(|| (0..strip_slots).map(|_| None).collect());
        let value = &mut strip[offset];
        let was_filled = value.is_some();
        let value = value.get_or_insert_with(make);
        if !was_filled {
            self.filled[slot / 64] |= 1 << (slot % 64);
            self.filled_count += 1;
        }
        value
    }

    fn remove(&mut self, key: &(u16, u16)) -> Option<V> {
        let Some(slot) = self.slot_of(*key) else {
            return if is_dense(*key) {
                None
            } else {
                self.reshaped = true;
                self.outside.remove(key)
            };
        };
        let (strip, offset) = self.strip_and_offset(slot);
        let slots = self.strips.get_mut(strip)?.as_mut()?;
        let value = slots[offset].take()?;
        note_slot(&mut self.changed_cells, self.side, slot);
        self.filled[slot / 64] &= !(1 << (slot % 64));
        self.filled_count -= 1;
        self.stamps.set(slot);
        Some(value)
    }

    /// Set `key`'s value as a commit does, through the insert or the removal
    /// a direct write makes, and stamp the cell even where it was and stays
    /// empty. Returns the value it held.
    fn replace(&mut self, key: (u16, u16), value: Option<V>) -> Option<V> {
        let old = self.remove(&key);
        if let Some(value) = value {
            self.get_or_insert_with(key, || value);
        }
        if let Some(slot) = self.slot_of(key) {
            self.stamps.set(slot);
        }
        old
    }

    /// Whether a write reached `key`'s cell since the pass began.
    fn stamped(&self, key: (u16, u16)) -> bool {
        !self.stamps.0.is_empty()
            && self
                .slot_of(key)
                .is_none_or(|slot| self.stamps.0[slot / 64] & (1 << (slot % 64)) != 0)
    }

    fn iter(&self) -> impl Iterator<Item = ((u16, u16), &V)> + '_ {
        let inside = self
            .filled
            .iter()
            .enumerate()
            .flat_map(|(word, &bits)| SetBits(bits).map(move |bit| word * 64 + bit))
            .map(|slot| {
                let value = self.slot(slot).and_then(Option::as_ref);
                (self.key_of(slot), value.expect("filled slot"))
            });
        merge_by_key(
            inside,
            self.outside.iter().map(|(&key, value)| (key, value)),
        )
    }

    /// Mutable entries in ascending `(x, y)` order, every one stamped.
    fn iter_mut(&mut self) -> impl Iterator<Item = ((u16, u16), &mut V)> + '_ {
        let (side, side_shift, strip_slots) = (self.side, self.side_shift(), self.strip_slots());
        self.reshaped = true;
        for (stamps, filled) in self.stamps.0.iter_mut().zip(&self.filled) {
            *stamps |= filled;
        }
        let inside = self
            .strips
            .iter_mut()
            .enumerate()
            .filter_map(|(strip, slots)| Some((strip, &mut **slots.as_mut()?)))
            .flat_map(move |(strip, slots)| {
                slots
                    .iter_mut()
                    .enumerate()
                    .filter_map(move |(offset, value)| {
                        let slot = strip * strip_slots + offset;
                        let key = ((slot >> side_shift) as u16, (slot & (side - 1)) as u16);
                        value.as_mut().map(|value| (key, value))
                    })
            });
        merge_by_key(
            inside,
            self.outside.iter_mut().map(|(&key, value)| (key, value)),
        )
    }
}

impl<V: Clone> View<V> {
    /// Where the fork's overlay holds `key`, if it wrote it.
    fn position(&self, key: (u16, u16)) -> Option<usize> {
        self.overlay.iter().position(|(written, _)| *written == key)
    }

    /// `key`'s value as the view reads it: the fork's, else the base's.
    #[inline(never)]
    fn get(&self, key: &(u16, u16)) -> Option<&V> {
        match self.overlay.iter().find(|(written, _)| written == key) {
            Some((_, value)) => value.as_ref(),
            None => self.base.get(key),
        }
    }

    /// The entries the view reads: the base's, less and plus the fork's.
    fn len(&self) -> usize {
        self.overlay
            .iter()
            .fold(self.base.len(), |len, (key, value)| {
                len + usize::from(value.is_some()) - usize::from(self.base.get(key).is_some())
            })
    }

    /// The fork's own copy of `key`'s cell, made from the base's value on the
    /// first write. An unwritten cell the base leaves empty gets one only
    /// when `create`: handing out or removing nothing writes nothing.
    fn entry(&mut self, key: (u16, u16), create: bool) -> Option<&mut Option<V>> {
        assert!(self.fork, "{SHARED_WRITE}");
        let index = match self.position(key) {
            Some(index) => index,
            None => {
                let value = self.base.get(&key).cloned();
                if value.is_none() && !create {
                    return None;
                }
                self.overlay.push((key, value));
                self.overlay.len() - 1
            }
        };
        Some(&mut self.overlay[index].1)
    }

    #[inline(never)]
    fn get_mut(&mut self, key: (u16, u16)) -> Option<&mut V> {
        self.entry(key, false)?.as_mut()
    }

    #[inline(never)]
    fn get_or_insert_with(&mut self, key: (u16, u16), make: impl FnOnce() -> V) -> &mut V {
        self.entry(key, true)
            .expect("created on the first write")
            .get_or_insert_with(make)
    }

    #[inline(never)]
    fn remove(&mut self, key: (u16, u16)) -> Option<V> {
        self.entry(key, false)?.take()
    }

    /// The cells the view reads, as an ordinary map holds them.
    fn copy(&self) -> Cells<V> {
        let mut cells = Cells::clone(&self.base);
        for (key, value) in &self.overlay {
            cells.replace(*key, value.clone());
        }
        cells
    }

    /// Entries in ascending `(x, y)` order: the base's, the fork's for the
    /// cells it wrote.
    fn iter(&self) -> impl Iterator<Item = ((u16, u16), &V)> + '_ {
        let mut written: Vec<_> = self
            .overlay
            .iter()
            .filter_map(|(key, value)| Some((*key, value.as_ref()?)))
            .collect();
        written.sort_unstable_by_key(|&(key, _)| key);
        let unwritten = self
            .base
            .iter()
            .filter(|&(key, _)| self.position(key).is_none());
        merge_by_key(unwritten, written.into_iter())
    }
}

/// Positions of the set bits of a word, lowest first.
struct SetBits(u64);

impl Iterator for SetBits {
    type Item = usize;

    fn next(&mut self) -> Option<usize> {
        (self.0 != 0).then(|| {
            let bit = self.0.trailing_zeros() as usize;
            self.0 &= self.0 - 1;
            bit
        })
    }
}

impl<V: Clone + PartialEq> PartialEq for CellMap<V> {
    fn eq(&self, other: &Self) -> bool {
        self.len() == other.len() && self.iter().eq(other.iter())
    }
}

impl<V: Clone + Eq> Eq for CellMap<V> {}

impl<V: Clone + fmt::Debug> fmt::Debug for CellMap<V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.iter()).finish()
    }
}

impl<V: Clone + Serialize> Serialize for CellMap<V> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // A shared map and a fork live only inside an object pass.
        assert!(
            self.owns_cells(),
            "save of a shared or forked cell map: unshare it first, and save no fork"
        );
        let mut map = serializer.serialize_map(Some(self.len()))?;
        for (key, value) in self.iter() {
            map.serialize_entry(&key, value)?;
        }
        map.end()
    }
}

impl<'de, V: Clone + Deserialize<'de>> Deserialize<'de> for CellMap<V> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct CellMapVisitor<V>(PhantomData<V>);

        impl<'de, V: Clone + Deserialize<'de>> Visitor<'de> for CellMapVisitor<V> {
            type Value = CellMap<V>;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a map from cell coordinates")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut access: A) -> Result<Self::Value, A::Error> {
                let mut map = CellMap::new();
                while let Some((key, value)) = access.next_entry::<(u16, u16), V>()? {
                    map.insert(key, value);
                }
                Ok(map)
            }
        }

        deserializer.deserialize_map(CellMapVisitor(PhantomData))
    }
}

#[cfg(all(test, tests_sim_c_g))]
mod tests {
    use super::CellMap;
    use std::collections::BTreeMap;

    /// One coordinate: the array's first and last index, the first index
    /// outside it, the largest key, a run across the first strip boundary
    /// (x = 8) and runs across the retail side, where an extended key grows
    /// the map, and across the largest array's edge.
    fn coordinate(n: u64) -> u16 {
        match n % 7 {
            0 => 0,
            1 => (super::SIDE - 1) as u16,
            2 => super::SIDE as u16,
            3 => u16::MAX,
            4 => (n / 7 % 16) as u16,
            5 => (super::RETAIL_SIDE as u16 - 12) + (n / 7 % 24) as u16,
            _ => (super::SIDE as u16 - 12) + (n / 7 % 24) as u16,
        }
    }

    #[test]
    fn behaves_like_a_sorted_map() {
        let mut cells: CellMap<u32> = CellMap::new();
        let mut sorted: BTreeMap<(u16, u16), u32> = BTreeMap::new();
        let mut state = 0x9e37_79b9_7f4a_7c15_u64;
        let mut removed = 0;
        for step in 0..20_000u32 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            // The operation and the two coordinates read disjoint bits.
            let at = (
                coordinate((state >> 8) & 0xffff),
                coordinate((state >> 24) & 0xffff),
            );
            match state % 4 {
                0 => assert_eq!(cells.insert(at, step), sorted.insert(at, step)),
                1 => {
                    let gone = cells.remove(&at);
                    assert_eq!(gone, sorted.remove(&at));
                    removed += usize::from(gone.is_some());
                }
                2 => {
                    *cells.get_or_insert_with(at, || 7) += 1;
                    *sorted.entry(at).or_insert(7) += 1;
                }
                _ => {
                    if let Some(value) = cells.get_mut(&at) {
                        *value += 2;
                    }
                    if let Some(value) = sorted.get_mut(&at) {
                        *value += 2;
                    }
                }
            }
            assert_eq!(cells.get(&at), sorted.get(&at));
            assert_eq!(cells.len(), sorted.len());
        }
        assert!(removed > 1000, "only {removed} removals found an entry");
        assert!(
            cells
                .iter()
                .map(|(key, &value)| (key, value))
                .eq(sorted.clone())
        );
        for value in cells.values_mut() {
            *value ^= 1;
        }
        for value in sorted.values_mut() {
            *value ^= 1;
        }
        assert!(
            cells
                .iter_mut()
                .map(|(key, value)| (key, *value))
                .eq(sorted.clone())
        );
        assert_eq!(format!("{cells:?}"), format!("{sorted:?}"));

        // Emptied by removals, the map is indistinguishable from a new one.
        for key in sorted.keys() {
            cells.remove(key);
        }
        assert!(cells == CellMap::new());
        assert_eq!(cells.len(), 0);
        assert_eq!(format!("{cells:?}"), "{}");
        assert_eq!(
            crate::util::save_codec::serialize(&cells).unwrap(),
            crate::util::save_codec::serialize(&BTreeMap::<(u16, u16), u32>::new()).unwrap()
        );
    }

    #[test]
    fn sync_brings_a_mirror_up_to_the_cells_written_since_its_last_copy() {
        let mut source: CellMap<u32> = CellMap::new();
        let mut mirror: CellMap<u32> = CellMap::new();
        assert!(source.get_mut(&(3, 4)).is_none());
        assert!(source.remove(&(3, 4)).is_none());
        assert!(source.own.changed_cells.is_empty());
        source.insert((3, 4), 1);
        source.sync_into(&mut mirror);
        assert_eq!(mirror.get(&(3, 4)), Some(&1));

        // One cell in a strip the mirror already holds, one in a strip it
        // has never allocated.
        source.insert((3, 5), 2);
        source.insert((200, 6), 3);
        source.sync_into(&mut mirror);
        assert_eq!(mirror.get(&(3, 4)), Some(&1));
        assert_eq!(mirror.get(&(3, 5)), Some(&2));
        assert_eq!(mirror.get(&(200, 6)), Some(&3));

        // An emptied cell clears in the mirror beside the cells it keeps.
        source.remove(&(3, 5));
        source.sync_into(&mut mirror);
        assert_eq!(mirror.get(&(3, 4)), Some(&1));
        assert_eq!(mirror.get(&(3, 5)), None);
        assert_eq!(mirror.get(&(200, 6)), Some(&3));
    }

    #[test]
    fn retail_keys_keep_the_retail_side_until_an_extended_key_arrives() {
        let mut cells: CellMap<u32> = CellMap::new();
        cells.insert((511, 3), 1);
        cells.insert((2, 511), 2);
        cells.insert((u16::MAX, 0), 3);
        assert_eq!(cells.own.side, super::RETAIL_SIDE);

        cells.insert((600, 1), 4);
        assert_eq!(cells.own.side, super::SIDE);
        assert_eq!(
            cells
                .iter()
                .map(|(key, &value)| (key, value))
                .collect::<Vec<_>>(),
            [
                ((2, 511), 2),
                ((511, 3), 1),
                ((600, 1), 4),
                ((u16::MAX, 0), 3)
            ]
        );
    }

    #[test]
    fn saves_the_same_bytes_as_a_sorted_map() {
        let entries = [
            ((3, 4), 1u64),
            ((511, 0), 2),
            ((0, 600), 3),
            ((700, 2), 4),
            ((3, 3), 5),
        ];
        let cells: CellMap<u64> =
            entries
                .iter()
                .copied()
                .fold(CellMap::new(), |mut map, (k, v)| {
                    map.insert(k, v);
                    map
                });
        let sorted: BTreeMap<(u16, u16), u64> = entries.into_iter().collect();
        let bytes = crate::util::save_codec::serialize(&cells).unwrap();
        assert_eq!(bytes, crate::util::save_codec::serialize(&sorted).unwrap());
        let restored: CellMap<u64> = crate::util::save_codec::deserialize(&bytes).unwrap();
        assert_eq!(restored, cells);
    }
}
