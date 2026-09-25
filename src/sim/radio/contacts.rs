//! `Contacts` — the sparse, capacity-bounded RadioClass contact slot store.
//!
//! A fixed-capacity array of `Option<u64>` (`RadioClass+0xE4` items, `+0xE8`
//! count): inserts fill the first null slot (no append-grow), removals null a
//! slot in place (no compaction, so slot positions are stable). A HELLO sender
//! with no free slot first sends OVER_OUT to slot 0 and then reuses it
//! (`0x0065AA1F..0x0065AA34`). Capacity is `max(NumberOfDocks, 1)` for
//! buildings, else 1. Navigation uses the contact slot index to select a
//! docking offset; admission reads membership through `contains`. Sparse slot
//! positions are therefore hash-relevant.
//! sim/ only — never render/ui/sidebar/audio/net.
use serde::{Deserialize, Serialize};
use std::hash::{Hash, Hasher};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Contacts {
    slots: Vec<Option<u64>>,
}

impl Default for Contacts {
    /// One empty slot — the default for a non-dock object (a single radio link).
    fn default() -> Self {
        Self { slots: vec![None] }
    }
}

impl Contacts {
    /// Construct with `n.max(1)` empty slots.
    pub fn with_capacity(n: usize) -> Self {
        Self {
            slots: vec![None; n.max(1)],
        }
    }

    /// Grow-only resize to `n.max(1)` slots — never shrinks, preserves contents.
    pub fn set_capacity(&mut self, n: usize) {
        let target = n.max(1);
        if target > self.slots.len() {
            self.slots.resize(target, None);
        }
    }

    /// Number of slots.
    #[inline]
    pub fn capacity(&self) -> usize {
        self.slots.len()
    }

    /// Membership test — the load-bearing `Can_Enter_Cell` reader.
    pub fn contains(&self, id: u64) -> bool {
        self.slots.iter().any(|s| *s == Some(id))
    }

    /// Slot index holding `id`, if any (the dock-pad index basis).
    pub fn find_slot(&self, id: u64) -> Option<usize> {
        self.slots.iter().position(|s| *s == Some(id))
    }

    /// Read slot `i` (for the hash fold / pad lookup).
    #[inline]
    pub fn slot(&self, i: usize) -> Option<u64> {
        self.slots.get(i).copied().flatten()
    }

    /// Count of filled slots.
    pub fn len(&self) -> usize {
        self.slots.iter().filter(|s| s.is_some()).count()
    }

    /// Whether no slot is filled.
    pub fn is_empty(&self) -> bool {
        self.slots.iter().all(|s| s.is_none())
    }

    /// Receiver-side first-null insert. Returns the slot used. Idempotent — an
    /// id already present returns its existing slot. `None` when full: a
    /// saturated receiver denies without evicting (the dock idiom).
    pub fn insert(&mut self, id: u64) -> Option<usize> {
        if let Some(existing) = self.find_slot(id) {
            return Some(existing);
        }
        let slot = self.slots.iter().position(|s| s.is_none())?;
        self.slots[slot] = Some(id);
        Some(slot)
    }

    /// First null slot, the one a HELLO fills (`0x0065AA02..0x0065AA0C`).
    pub fn first_free(&self) -> Option<usize> {
        self.slots.iter().position(|s| s.is_none())
    }

    /// Store `id` in slot `i` (a HELLO sender's ROGER, `0x0065AA5A`).
    pub fn set_slot(&mut self, i: usize, id: u64) {
        if let Some(slot) = self.slots.get_mut(i) {
            *slot = Some(id);
        }
    }

    /// `RadioClass @ 0x0065ADF0`: some slot is null or already holds `id`.
    pub fn has_free_or(&self, id: u64) -> bool {
        self.slots.iter().any(|s| s.is_none() || *s == Some(id))
    }

    /// BREAK: null the first slot holding `id` (no compaction). Returns the slot.
    pub fn remove(&mut self, id: u64) -> Option<usize> {
        let slot = self.find_slot(id)?;
        self.slots[slot] = None;
        Some(slot)
    }

    /// Null every slot, preserving capacity (teardown / limbo).
    pub fn clear_all(&mut self) {
        for s in &mut self.slots {
            *s = None;
        }
    }

    /// Live ids in slot order (skips holes) — broadcast-BREAK order.
    pub fn iter_live(&self) -> impl Iterator<Item = u64> + '_ {
        self.slots.iter().filter_map(|s| *s)
    }

    /// Deterministic hash fold: capacity, then each slot's `Option` by index.
    /// Null holes and pad-bearing positions both matter, so slot position is
    /// part of the pre-image.
    pub fn hash_fold<H: Hasher>(&self, hasher: &mut H) {
        (self.slots.len() as u32).hash(hasher);
        for s in &self.slots {
            match s {
                Some(id) => {
                    1u8.hash(hasher);
                    id.hash(hasher);
                }
                None => 0u8.hash(hasher),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_one_empty_slot() {
        let c = Contacts::default();
        assert_eq!(c.capacity(), 1);
        assert!(c.is_empty());
        assert_eq!(c.len(), 0);
    }

    #[test]
    fn first_null_insert_returns_slot_and_is_idempotent() {
        let mut c = Contacts::with_capacity(3);
        assert_eq!(c.insert(7), Some(0));
        assert_eq!(c.insert(8), Some(1));
        assert_eq!(c.insert(7), Some(0)); // idempotent → existing slot
        assert_eq!(c.len(), 2);
        assert!(c.contains(7) && c.contains(8));
    }

    #[test]
    fn receiver_full_insert_returns_none_no_evict() {
        let mut c = Contacts::with_capacity(1);
        assert_eq!(c.insert(7), Some(0));
        assert_eq!(c.insert(8), None); // full → denied
        assert!(c.contains(7) && !c.contains(8)); // 7 not evicted
    }

    #[test]
    fn free_slot_probe_and_direct_store() {
        let mut c = Contacts::with_capacity(2);
        c.insert(7);
        assert_eq!(c.first_free(), Some(1));
        assert!(c.has_free_or(9));
        c.set_slot(1, 8);
        assert_eq!(c.first_free(), None);
        assert!(c.has_free_or(8) && !c.has_free_or(9));
    }

    #[test]
    fn remove_nulls_in_place_no_compaction() {
        let mut c = Contacts::with_capacity(3);
        c.insert(7);
        c.insert(8);
        c.insert(9);
        assert_eq!(c.remove(8), Some(1));
        // Slot 1 is now a hole; 7 and 9 keep their positions.
        assert_eq!(c.find_slot(7), Some(0));
        assert_eq!(c.find_slot(9), Some(2));
        assert!(!c.contains(8));
        assert_eq!(c.iter_live().collect::<Vec<_>>(), vec![7, 9]);
    }

    #[test]
    fn set_capacity_grows_only() {
        let mut c = Contacts::with_capacity(2);
        c.insert(7);
        c.set_capacity(4);
        assert_eq!(c.capacity(), 4);
        assert!(c.contains(7));
        c.set_capacity(1); // never shrinks
        assert_eq!(c.capacity(), 4);
    }

    #[test]
    fn clear_all_preserves_capacity() {
        let mut c = Contacts::with_capacity(3);
        c.insert(7);
        c.insert(8);
        c.clear_all();
        assert!(c.is_empty());
        assert_eq!(c.capacity(), 3);
    }
}
