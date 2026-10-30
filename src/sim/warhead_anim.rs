//! Phobos `WHAnimRemainingCreationInterval` runtime counters.
//!
//! Phobos stores one counter on `TechnoExt` for every interval-bearing warhead
//! a Techno fires, and one on `WarheadTypeExt` for warheads with no firing
//! Techno. Both use the same gate: reset to the configured interval and create
//! when the counter is nonpositive (or the interval is below one); otherwise
//! decrement and skip this detonation.
//!
//! ## Dependency rules
//! - Part of sim/ — no render/ui/audio/net dependencies.

use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};

use crate::sim::intern::InternedId;

/// Per-warhead-type counters for detonations whose Bullet has no live Techno
/// owner. The owning Simulation serializes and hashes this record.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct WarheadAnimRuntime {
    remaining: BTreeMap<InternedId, i32>,
}

impl WarheadAnimRuntime {
    /// Gate one detonation against the per-type counter.
    pub(crate) fn interval_allows(&mut self, warhead: InternedId, interval: i32) -> bool {
        // An interval below one always creates and resets to its own value;
        // dropping the entry keeps the empty map equal to Phobos' untouched
        // default counter of 0.
        if interval < 1 {
            self.remaining.remove(&warhead);
            return true;
        }
        interval_allows(self.remaining.entry(warhead).or_insert(0), interval)
    }

    /// Hash only live counters, so an untouched world contributes no bytes.
    pub(crate) fn hash_state(&self, hasher: &mut impl Hasher) {
        if self.remaining.is_empty() {
            return;
        }
        b"phobos-warhead-anim-intervals-v1".hash(hasher);
        self.remaining.len().hash(hasher);
        for (warhead, remaining) in &self.remaining {
            warhead.hash(hasher);
            remaining.hash(hasher);
        }
    }
}

/// Shared gate used by both counter owners. A nonpositive counter creates and
/// resets; a positive one skips and decrements.
pub(crate) fn interval_allows(counter: &mut i32, interval: i32) -> bool {
    if interval < 1 {
        *counter = interval;
        true
    } else if *counter <= 0 {
        *counter = interval;
        true
    } else {
        *counter -= 1;
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interval_gate_resets_creates_and_decrements() {
        let mut counter = 0;
        assert!(interval_allows(&mut counter, 0));
        assert_eq!(counter, 0);
        assert!(interval_allows(&mut counter, 2));
        assert_eq!(counter, 2);
        assert!(!interval_allows(&mut counter, 2));
        assert_eq!(counter, 1);
        assert!(!interval_allows(&mut counter, 2));
        assert_eq!(counter, 0);
        assert!(interval_allows(&mut counter, 2));
        assert_eq!(counter, 2);
        assert!(interval_allows(&mut counter, -1));
        assert_eq!(counter, -1);
    }

    #[test]
    fn zero_interval_does_not_retain_a_counter() {
        let warhead = InternedId::from_index(0);
        let mut runtime = WarheadAnimRuntime::default();
        assert!(runtime.interval_allows(warhead, 0));
        assert!(runtime.remaining.is_empty());
        assert!(runtime.interval_allows(warhead, 2));
        assert_eq!(runtime.remaining.get(&warhead), Some(&2));
        assert!(!runtime.interval_allows(warhead, 2));
        assert!(runtime.interval_allows(warhead, 0));
        assert!(runtime.remaining.is_empty());
    }
}
