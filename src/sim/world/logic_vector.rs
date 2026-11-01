//! The LogicClass active-object vector: the authority on AI visitation order.
//!
//! Owns an insertion-ordered list of stable_ids. Tail-append on reveal,
//! order-preserving compacting remove on conceal, no sort. Membership itself is
//! a flag on each object ([`LogicMembership`] on a Techno); [`LogicVector`]
//! owns only the order. Serializes transparently as its inner `Vec<u64>` so the
//! saved order is restored verbatim.
//!
//! Dependency rules: part of sim/ — depends only on std, serde, the crate's
//! lookup hasher and the live-read check switch.

use crate::util::lookup_hash::LookupMap;

/// `ObjectClass+0x98`, IsInLogic: the object is in LogicClass's active vector.
/// The ObjectClass constructor clears it (`0x005F398D`);
/// `LogicClass::RegisterObject @ 0x0055BAA0` sets it after a successful
/// insert and `LogicClass::UnregisterObject @ 0x0055BAE0` clears it. The
/// world's register and unregister steps, the construction boundary and the
/// load rebuild write it ([`Self::mark`]). Not saved: a load rebuilds it from
/// the saved vector order (`Simulation::rebuild_logic_membership`); exact
/// native save/load reconstruction remains unverified.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LogicMembership {
    member: bool,
}

impl LogicMembership {
    /// The object is in the active vector.
    pub const fn is_member(self) -> bool {
        self.member
    }

    /// RegisterObject's set or UnregisterObject's clear.
    pub(in crate::sim::world) fn mark(&mut self, member: bool) {
        self.member = member;
    }

    /// Install a fixture's membership.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_for_test(&mut self, member: bool) {
        self.member = member;
    }
}

/// Insertion-ordered, membership-gated active-object order.
#[derive(Debug, Default, Clone)]
pub struct LogicVector {
    order: Vec<u64>,
    /// VERA-only: where [`Self::remove_first`] finds an id's first slot
    /// without walking the order. `order` is the source of truth; every
    /// writer below keeps this beside it, and a load or test order rebuilds
    /// it ([`Self::from_order`]). Never saved or hashed.
    slots: SlotIndex,
    /// Deterministic seam for the lifecycle transaction's append-failure test.
    /// Never exists in production builds and is never serialized.
    #[cfg(any(test, feature = "test-support"))]
    fail_next_insert: bool,
}

/// Each slot's append number, and each id's first slot's, so a binary search
/// finds the slot.
#[derive(Debug, Default, Clone)]
struct SlotIndex {
    /// Each slot's append number, beside `order`. An append takes the next
    /// number and a removal keeps the others', so the column ascends.
    appended: Vec<u64>,
    /// Each stored id's first slot's append number.
    first: LookupMap<u64, u64>,
    /// Slots whose id an earlier slot holds. Registration is gated on the
    /// membership flag and a load rejects a repeated id, so only fixtures
    /// make one; the removal of an id with a repeat walks to its next slot.
    repeats: usize,
    /// The next append's number.
    next: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LogicInsertError {
    Capacity,
    #[cfg(any(test, feature = "test-support"))]
    ForcedTestFailure,
}

impl LogicVector {
    pub fn new() -> Self {
        Self::default()
    }

    /// Fallible tail-append. The caller sets object-local membership only after
    /// this succeeds; allocation failure must not roll back an already-successful
    /// cell Mark transaction.
    ///
    /// gamemd-derived: active YR's unsorted LogicClass path calls
    /// `LayerClass__Insert @ 0x005519B0` with a zero sort argument, which
    /// appends the new element at the current tail after capacity succeeds.
    pub(crate) fn try_push(&mut self, id: u64) -> Result<(), LogicInsertError> {
        #[cfg(any(test, feature = "test-support"))]
        if std::mem::take(&mut self.fail_next_insert) {
            return Err(LogicInsertError::ForcedTestFailure);
        }

        let capacity = |_| LogicInsertError::Capacity;
        self.order.try_reserve(1).map_err(capacity)?;
        self.slots.appended.try_reserve(1).map_err(capacity)?;
        self.slots.first.try_reserve(1).map_err(capacity)?;
        self.order.push(id);
        self.slots.append(id);
        Ok(())
    }

    /// Remove the first matching slot and compact later entries left. This
    /// deliberately does not remove duplicates beyond the first match.
    ///
    /// gamemd-derived: active YR `LogicClass__UnregisterObject @ 0x0055BAE0`
    /// removes only the first matching vector slot and preserves later order.
    /// VERA-only: the slot comes from a binary search of the append numbers
    /// instead of a walk; debug builds check it against the walk (the
    /// live-read check).
    pub(crate) fn remove_first(&mut self, id: u64) -> bool {
        let Some(number) = self.slots.first.remove(&id) else {
            debug_assert!(
                !crate::sim::touch_log::live_read_check_enabled() || !self.order.contains(&id),
                "the Logic slot index misses {id}, which the order holds"
            );
            return false;
        };
        let index = self
            .slots
            .appended
            .binary_search(&number)
            .expect("an id's first slot is stored");
        debug_assert!(
            !crate::sim::touch_log::live_read_check_enabled()
                || self.order.iter().position(|&candidate| candidate == id) == Some(index),
            "the Logic slot index disagrees with the order for {id}"
        );
        self.order.remove(index);
        self.slots.appended.remove(index);
        if self.slots.repeats > 0
            && let Some(offset) = self.order[index..].iter().position(|&other| other == id)
        {
            self.slots
                .first
                .insert(id, self.slots.appended[index + offset]);
            self.slots.repeats -= 1;
        }
        true
    }

    /// The order verbatim, with its slot index rebuilt.
    fn from_order(order: Vec<u64>) -> Self {
        let mut slots = SlotIndex::default();
        for &id in &order {
            slots.append(id);
        }
        Self {
            order,
            slots,
            #[cfg(any(test, feature = "test-support"))]
            fail_next_insert: false,
        }
    }

    /// The order verbatim — no sorted fallback, no filtering.
    pub fn snapshot(&self) -> Vec<u64> {
        self.order.clone()
    }

    /// Borrow the order for hashing / iteration.
    pub fn as_slice(&self) -> &[u64] {
        &self.order
    }

    pub fn len(&self) -> usize {
        self.order.len()
    }

    /// Test-only: force a specific order (e.g. opposite stable-id order).
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_order_for_test(&mut self, order: Vec<u64>) {
        let fail_next_insert = self.fail_next_insert;
        *self = Self::from_order(order);
        self.fail_next_insert = fail_next_insert;
    }

    /// Test-only: make exactly the next append fail before reserving or mutating.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn force_next_insert_failure_for_test(&mut self) {
        self.fail_next_insert = true;
    }
}

impl serde::Serialize for LogicVector {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.order.serialize(serializer)
    }
}

impl<'de> serde::Deserialize<'de> for LogicVector {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Self::from_order(Vec::<u64>::deserialize(deserializer)?))
    }
}

impl SlotIndex {
    /// Number a slot appended for `id`.
    fn append(&mut self, id: u64) {
        self.appended.push(self.next);
        match self.first.entry(id) {
            std::collections::hash_map::Entry::Vacant(first) => {
                first.insert(self.next);
            }
            std::collections::hash_map::Entry::Occupied(_) => self.repeats += 1,
        }
        self.next += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unregister_removes_only_first_matching_slot() {
        let mut v = LogicVector::new();
        v.set_order_for_test(vec![10, 20, 20, 30]);
        assert!(v.remove_first(20));
        assert_eq!(v.snapshot(), vec![10, 20, 30]);
    }

    /// The slot index finds each id's first slot after re-registration at the
    /// tail, removals ahead of it, a load and a repeated id.
    #[test]
    fn remove_first_finds_the_first_slot_through_appends_removals_and_loads() {
        let mut v = LogicVector::new();
        for id in 1..=5 {
            v.try_push(id).unwrap();
        }
        assert!(v.remove_first(3));
        v.try_push(3).unwrap();
        assert!(v.remove_first(1));
        assert!(!v.remove_first(1));
        assert!(v.remove_first(4));
        assert_eq!(v.snapshot(), [2, 5, 3]);

        let mut loaded: LogicVector = serde_json::from_str("[7, 2, 7, 3, 7]").unwrap();
        assert!(loaded.remove_first(7));
        assert!(loaded.remove_first(3));
        assert!(loaded.remove_first(7));
        assert_eq!(loaded.snapshot(), [2, 7]);
        loaded.try_push(7).unwrap();
        assert!(loaded.remove_first(7));
        assert_eq!(loaded.snapshot(), [2, 7]);
    }
}
