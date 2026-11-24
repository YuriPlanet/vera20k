//! Parallel digests over [`StateHasher`](crate::util::state_hasher::StateHasher).
//!
//! The hasher itself lives in `util/` so `rules` can hash its own identities
//! without depending on `sim`; only this rayon-backed batch helper needs the
//! simulation's worker pool.

use std::hash::Hasher;

use crate::util::state_hasher::StateHasher;

/// One [`StateHasher`] digest per item, each fed by `fold`. With `parallel`
/// the items spread over scoped threads; a digest depends on its item alone,
/// so the result never depends on the thread count.
pub(crate) fn digests<T: Sync>(
    items: &[T],
    parallel: bool,
    fold: impl Fn(&T, &mut StateHasher) + Sync,
) -> Vec<u64> {
    let digest = |item: &T| {
        let mut hasher = StateHasher::new();
        fold(item, &mut hasher);
        hasher.finish()
    };
    if parallel {
        crate::sim::parallel::map(items, digest)
    } else {
        items.iter().map(digest).collect()
    }
}

#[cfg(all(test, tests_sim_p_s))]
mod tests {
    use super::digests;
    use crate::util::state_hasher::StateHasher;
    use std::hash::{Hash, Hasher};

    #[test]
    fn parallel_digests_match_serial_ones() {
        let items: Vec<(u64, Vec<u8>)> = (0..1000)
            .map(|n| (n, vec![n as u8; n as usize % 70]))
            .collect();
        let fold = |item: &(u64, Vec<u8>), hasher: &mut StateHasher| item.hash(hasher);
        let serial = digests(&items, false, fold);
        assert_eq!(digests(&items, true, fold), serial);

        let mut single = StateHasher::new();
        items[7].hash(&mut single);
        assert_eq!(serial[7], single.finish());
    }
}
