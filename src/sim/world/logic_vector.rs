//! The LogicClass active-object vector: the authority on AI visitation order.
//!
//! Owns an insertion-ordered list of stable_ids. Tail-append on reveal,
//! order-preserving compacting remove on conceal, no sort. Membership itself is
//! tracked by a flag on each entity (see `GameEntity::in_logic_vector`); this type
//! owns only the order. Serializes transparently as its inner `Vec<u64>` so the
//! saved order is restored verbatim.
//!
//! Dependency rules: part of sim/ — depends only on std + serde.

/// Insertion-ordered, membership-gated active-object order.
#[derive(Debug, Default, Clone)]
pub struct LogicVector {
    order: Vec<u64>,
    /// Deterministic seam for the lifecycle transaction's append-failure test.
    /// Never exists in production builds and is never serialized.
    #[cfg(test)]
    fail_next_insert: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LogicInsertError {
    Capacity,
    #[cfg(test)]
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
    /// `DynamicVector__Insert @ 0x005519B0` with a zero sort argument, which
    /// appends the new element at the current tail after capacity succeeds.
    pub(crate) fn try_push(&mut self, id: u64) -> Result<(), LogicInsertError> {
        #[cfg(test)]
        if std::mem::take(&mut self.fail_next_insert) {
            return Err(LogicInsertError::ForcedTestFailure);
        }

        self.order
            .try_reserve(1)
            .map_err(|_| LogicInsertError::Capacity)?;
        self.order.push(id);
        Ok(())
    }

    /// Remove the first matching slot and compact later entries left. This
    /// deliberately does not remove duplicates beyond the first match.
    ///
    /// gamemd-derived: active YR `LogicClass__UnregisterObject @ 0x0055BAE0`
    /// removes only the first matching vector slot and preserves later order.
    pub(crate) fn remove_first(&mut self, id: u64) -> bool {
        let Some(index) = self.order.iter().position(|&candidate| candidate == id) else {
            return false;
        };
        self.order.remove(index);
        true
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
    #[cfg(test)]
    pub fn set_order_for_test(&mut self, order: Vec<u64>) {
        self.order = order;
    }

    /// Test-only: make exactly the next append fail before reserving or mutating.
    #[cfg(test)]
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
        let order = Vec::<u64>::deserialize(deserializer)?;
        Ok(Self {
            order,
            #[cfg(test)]
            fail_next_insert: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_appends_to_tail_no_sort() {
        let mut v = LogicVector::new();
        v.try_push(5).unwrap();
        v.try_push(1).unwrap();
        v.try_push(3).unwrap();
        assert_eq!(v.snapshot(), vec![5, 1, 3]); // insertion order, not sorted
    }

    #[test]
    fn unregister_preserves_order_compacting() {
        let mut v = LogicVector::new();
        v.try_push(10).unwrap();
        v.try_push(20).unwrap();
        v.try_push(30).unwrap();
        assert!(v.remove_first(20));
        assert_eq!(v.snapshot(), vec![10, 30]); // left-shift, tail preserved
    }

    #[test]
    fn unregister_absent_id_is_safe() {
        let mut v = LogicVector::new();
        v.try_push(1).unwrap();
        assert!(!v.remove_first(99));
        assert_eq!(v.snapshot(), vec![1]);
    }

    #[test]
    fn unregister_removes_only_first_matching_slot() {
        let mut v = LogicVector::new();
        v.set_order_for_test(vec![10, 20, 20, 30]);
        assert!(v.remove_first(20));
        assert_eq!(v.snapshot(), vec![10, 20, 30]);
    }

    #[test]
    fn snapshot_is_order_verbatim() {
        let mut v = LogicVector::new();
        v.try_push(7).unwrap();
        v.try_push(2).unwrap();
        assert_eq!(v.snapshot(), v.as_slice().to_vec());
    }

    #[test]
    fn serde_roundtrip_preserves_order() {
        let mut v = LogicVector::new();
        v.try_push(9).unwrap();
        v.try_push(4).unwrap();
        v.try_push(6).unwrap();
        let bytes = bincode::serialize(&v).expect("serialize");
        let back: LogicVector = bincode::deserialize(&bytes).expect("deserialize");
        assert_eq!(back.snapshot(), vec![9, 4, 6]);
    }

    #[test]
    fn forced_insert_failure_is_one_shot_and_non_mutating() {
        let mut v = LogicVector::new();
        v.try_push(1).unwrap();
        v.force_next_insert_failure_for_test();

        assert_eq!(v.try_push(2), Err(LogicInsertError::ForcedTestFailure));
        assert_eq!(v.snapshot(), vec![1]);
        assert_eq!(v.try_push(2), Ok(()));
        assert_eq!(v.snapshot(), vec![1, 2]);
    }
}
