//! Order-preserving work over rayon's persistent worker pool. Each result
//! depends on its item alone, so the output never depends on the thread
//! count or on scheduling. The pool's threads persist, so their thread-local
//! scratch (the path search workspaces) stays allocated between frames.

use rayon::prelude::*;

/// `work` over every item, results in item order. Workers steal items from
/// each other, so slower cores (efficiency cores, a busy host) take fewer.
/// A worker's panic resumes on the caller.
pub(crate) fn map<T: Sync, R: Send>(items: &[T], work: impl Fn(&T) -> R + Sync) -> Vec<R> {
    // A shared reference to a `Sync` closure is `Send`.
    items.par_iter().map(&work).collect()
}

/// The indices below `len` that `keep` selects, in ascending order.
pub(crate) fn select_indices(len: usize, keep: impl Fn(usize) -> bool + Sync) -> Vec<usize> {
    (0..len)
        .into_par_iter()
        .filter(|&index| keep(index))
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn results_keep_item_order() {
        let items: Vec<u64> = (0..1000).collect();
        let squares = super::map(&items, |item| item * item);
        assert_eq!(
            squares,
            items.iter().map(|item| item * item).collect::<Vec<_>>()
        );
        assert!(super::map(&[] as &[u64], |item| *item).is_empty());
        assert_eq!(
            super::select_indices(1000, |index| index % 7 == 3),
            (0..1000).filter(|index| index % 7 == 3).collect::<Vec<_>>()
        );
    }
}
