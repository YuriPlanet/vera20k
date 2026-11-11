//! A map keyed by native cell coordinates, dense over gamemd's cell array.

use std::collections::BTreeMap;
use std::fmt;
use std::marker::PhantomData;

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

/// A `BTreeMap<(u16, u16), V>` with O(1) lookups: each native cell has a
/// dense slot.
///
/// It keeps the sorted map's observable behavior: iteration visits keys in
/// ascending `(x, y)` order, and equality, `Debug` and serde see the same
/// entries (serde writes the same map, so saves stay byte-identical). Slots
/// are x-major (`x * 512 + y`), so key order is storage order, and a bitset
/// of filled slots lets iteration skip empty runs. Storage comes in strips of
/// eight columns, allocated on their first insert, so a small map pays for
/// the columns it uses. The dense side is the retail 512 until a key of an
/// extended map's larger array arrives, which relays it out once at 1024, so
/// retail maps keep their footprint. Keys outside the largest array, which no
/// real cell produces, keep a sorted map of their own.
#[derive(Clone)]
pub(crate) struct CellMap<V> {
    /// Dense cells per side: [`RETAIL_SIDE`], or [`SIDE`] once grown.
    side: usize,
    /// Shared copy-on-write with the presentation frame (docs/sim-thread.md):
    /// a write copies only its own row.
    strips: Vec<Option<std::sync::Arc<[Option<V>]>>>,
    /// One bit per slot, set while the slot holds a value.
    filled: Vec<u64>,
    filled_count: usize,
    outside: BTreeMap<(u16, u16), V>,
}

impl<V> Default for CellMap<V> {
    fn default() -> Self {
        Self {
            side: RETAIL_SIDE,
            strips: Vec::new(),
            filled: Vec::new(),
            filled_count: 0,
            outside: BTreeMap::new(),
        }
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

impl<V: Clone> CellMap<V> {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn len(&self) -> usize {
        self.filled_count + self.outside.len()
    }

    /// Slots per strip: one column of the dense array, so a write shared
    /// with a presentation frame copies only that column.
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

    fn slot_mut(&mut self, slot: usize) -> Option<&mut Option<V>> {
        let (strip, offset) = self.strip_and_offset(slot);
        let strip = std::sync::Arc::make_mut(self.strips.get_mut(strip)?.as_mut()?);
        Some(&mut strip[offset])
    }

    /// Relay the dense storage out at the largest side. Runs once, on the
    /// first key of an extended map's array beyond the retail side.
    fn grow(&mut self) {
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
    }

    pub(crate) fn get(&self, key: &(u16, u16)) -> Option<&V> {
        match self.slot_of(*key) {
            Some(slot) => self.slot(slot)?.as_ref(),
            None if is_dense(*key) => None,
            None => self.outside.get(key),
        }
    }

    pub(crate) fn get_mut(&mut self, key: &(u16, u16)) -> Option<&mut V> {
        match self.slot_of(*key) {
            Some(slot) => self.slot_mut(slot)?.as_mut(),
            None if is_dense(*key) => None,
            None => self.outside.get_mut(key),
        }
    }

    /// The value at `key`, inserting `make()` first when the cell is empty.
    pub(crate) fn get_or_insert_with(
        &mut self,
        key: (u16, u16),
        make: impl FnOnce() -> V,
    ) -> &mut V {
        if !is_dense(key) {
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
        }
        let strip = std::sync::Arc::make_mut(
            self.strips[strip].get_or_insert_with(|| (0..strip_slots).map(|_| None).collect()),
        );
        let value = &mut strip[offset];
        let was_filled = value.is_some();
        let value = value.get_or_insert_with(make);
        if !was_filled {
            self.filled[slot / 64] |= 1 << (slot % 64);
            self.filled_count += 1;
        }
        value
    }

    pub(crate) fn insert(&mut self, key: (u16, u16), value: V) -> Option<V> {
        let previous = self.remove(&key);
        self.get_or_insert_with(key, || value);
        previous
    }

    pub(crate) fn remove(&mut self, key: &(u16, u16)) -> Option<V> {
        let Some(slot) = self.slot_of(*key) else {
            return if is_dense(*key) {
                None
            } else {
                self.outside.remove(key)
            };
        };
        let value = self.slot_mut(slot)?.take()?;
        self.filled[slot / 64] &= !(1 << (slot % 64));
        self.filled_count -= 1;
        Some(value)
    }

    /// Entries in ascending `(x, y)` order.
    pub(crate) fn iter(&self) -> impl Iterator<Item = ((u16, u16), &V)> + '_ {
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

    /// Mutable entries in ascending `(x, y)` order.
    fn iter_mut(&mut self) -> impl Iterator<Item = ((u16, u16), &mut V)> + '_ {
        let (side, side_shift, strip_slots) = (self.side, self.side_shift(), self.strip_slots());
        let inside = self
            .strips
            .iter_mut()
            .enumerate()
            .filter_map(|(strip, slots)| Some((strip, std::sync::Arc::make_mut(slots.as_mut()?))))
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

    pub(crate) fn values(&self) -> impl Iterator<Item = &V> + '_ {
        self.iter().map(|(_, value)| value)
    }

    pub(crate) fn values_mut(&mut self) -> impl Iterator<Item = &mut V> + '_ {
        self.iter_mut().map(|(_, value)| value)
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

#[cfg(test)]
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
    fn retail_keys_keep_the_retail_side_until_an_extended_key_arrives() {
        let mut cells: CellMap<u32> = CellMap::new();
        cells.insert((511, 3), 1);
        cells.insert((2, 511), 2);
        cells.insert((u16::MAX, 0), 3);
        assert_eq!(cells.side, super::RETAIL_SIDE);

        cells.insert((600, 1), 4);
        assert_eq!(cells.side, super::SIDE);
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
