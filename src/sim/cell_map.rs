//! A map keyed by native cell coordinates, dense over gamemd's cell array.

use std::collections::BTreeMap;
use std::fmt;
use std::marker::PhantomData;

use serde::de::{Deserialize, Deserializer, MapAccess, Visitor};
use serde::ser::{Serialize, SerializeMap, Serializer};

/// Cells per side of gamemd's fixed cell array.
const SIDE: usize = crate::map::cell_index::CELL_ROW_STRIDE as usize;
/// Slots per strip: eight columns of the array.
const STRIP_SLOTS: usize = 8 * SIDE;

/// A `BTreeMap<(u16, u16), V>` with O(1) lookups: each native cell has a
/// dense slot.
///
/// It keeps the sorted map's observable behavior: iteration visits keys in
/// ascending `(x, y)` order, and equality, `Debug` and serde see the same
/// entries (serde writes the same map, so saves stay byte-identical). Slots
/// are x-major (`x * 512 + y`), so key order is storage order, and a bitset
/// of filled slots lets iteration skip empty runs. Storage comes in strips of
/// eight columns, allocated on their first insert, so a small map pays for
/// the columns it uses. Keys outside the native array, which no real cell
/// produces, keep a sorted map of their own.
#[derive(Clone)]
pub(crate) struct CellMap<V> {
    strips: Vec<Option<Box<[Option<V>]>>>,
    /// One bit per slot, set while the slot holds a value.
    filled: Vec<u64>,
    filled_count: usize,
    outside: BTreeMap<(u16, u16), V>,
}

impl<V> Default for CellMap<V> {
    fn default() -> Self {
        Self {
            strips: Vec::new(),
            filled: Vec::new(),
            filled_count: 0,
            outside: BTreeMap::new(),
        }
    }
}

fn slot_of((x, y): (u16, u16)) -> Option<usize> {
    let (x, y) = (usize::from(x), usize::from(y));
    (x < SIDE && y < SIDE).then_some(x * SIDE + y)
}

fn key_of(slot: usize) -> (u16, u16) {
    ((slot / SIDE) as u16, (slot % SIDE) as u16)
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

impl<V> CellMap<V> {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn len(&self) -> usize {
        self.filled_count + self.outside.len()
    }

    fn slot(&self, slot: usize) -> Option<&Option<V>> {
        let strip = self.strips.get(slot / STRIP_SLOTS)?.as_deref()?;
        Some(&strip[slot % STRIP_SLOTS])
    }

    fn slot_mut(&mut self, slot: usize) -> Option<&mut Option<V>> {
        let strip = self.strips.get_mut(slot / STRIP_SLOTS)?.as_deref_mut()?;
        Some(&mut strip[slot % STRIP_SLOTS])
    }

    pub(crate) fn get(&self, key: &(u16, u16)) -> Option<&V> {
        match slot_of(*key) {
            Some(slot) => self.slot(slot)?.as_ref(),
            None => self.outside.get(key),
        }
    }

    pub(crate) fn get_mut(&mut self, key: &(u16, u16)) -> Option<&mut V> {
        match slot_of(*key) {
            Some(slot) => self.slot_mut(slot)?.as_mut(),
            None => self.outside.get_mut(key),
        }
    }

    /// The value at `key`, inserting `make()` first when the cell is empty.
    pub(crate) fn get_or_insert_with(
        &mut self,
        key: (u16, u16),
        make: impl FnOnce() -> V,
    ) -> &mut V {
        let Some(slot) = slot_of(key) else {
            return self.outside.entry(key).or_insert_with(make);
        };
        if self.strips.is_empty() {
            self.strips.resize_with(SIDE * SIDE / STRIP_SLOTS, || None);
            self.filled = vec![0; SIDE * SIDE / 64];
        }
        let strip = self.strips[slot / STRIP_SLOTS]
            .get_or_insert_with(|| (0..STRIP_SLOTS).map(|_| None).collect());
        let value = &mut strip[slot % STRIP_SLOTS];
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
        if slot_of(key).is_some() {
            self.get_or_insert_with(key, || value);
        } else {
            self.outside.insert(key, value);
        }
        previous
    }

    pub(crate) fn remove(&mut self, key: &(u16, u16)) -> Option<V> {
        let Some(slot) = slot_of(*key) else {
            return self.outside.remove(key);
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
                (key_of(slot), value.expect("filled slot"))
            });
        merge_by_key(
            inside,
            self.outside.iter().map(|(&key, value)| (key, value)),
        )
    }

    /// Mutable entries in ascending `(x, y)` order.
    fn iter_mut(&mut self) -> impl Iterator<Item = ((u16, u16), &mut V)> + '_ {
        let inside = self
            .strips
            .iter_mut()
            .enumerate()
            .filter_map(|(strip, slots)| Some((strip, slots.as_deref_mut()?)))
            .flat_map(|(strip, slots)| {
                slots
                    .iter_mut()
                    .enumerate()
                    .filter_map(move |(offset, value)| {
                        let key = key_of(strip * STRIP_SLOTS + offset);
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

impl<V: PartialEq> PartialEq for CellMap<V> {
    fn eq(&self, other: &Self) -> bool {
        self.len() == other.len() && self.iter().eq(other.iter())
    }
}

impl<V: Eq> Eq for CellMap<V> {}

impl<V: fmt::Debug> fmt::Debug for CellMap<V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.iter()).finish()
    }
}

impl<V: Serialize> Serialize for CellMap<V> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.len()))?;
        for (key, value) in self.iter() {
            map.serialize_entry(&key, value)?;
        }
        map.end()
    }
}

impl<'de, V: Deserialize<'de>> Deserialize<'de> for CellMap<V> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct CellMapVisitor<V>(PhantomData<V>);

        impl<'de, V: Deserialize<'de>> Visitor<'de> for CellMapVisitor<V> {
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
    /// (x = 8) and a run across the array's edge.
    fn coordinate(n: u64) -> u16 {
        match n % 6 {
            0 => 0,
            1 => 511,
            2 => 512,
            3 => u16::MAX,
            4 => (n / 6 % 16) as u16,
            _ => 500 + (n / 6 % 24) as u16,
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
