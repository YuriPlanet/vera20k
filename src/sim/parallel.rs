//! Order-preserving work over rayon's persistent worker pool. Each result
//! depends on its item alone, so the output never depends on the thread
//! count or on scheduling. Work run [`alongside`] the caller and the caller
//! must only read what they share: the borrow checker rules out a shared
//! `&mut`, not interior mutability such as the shared cell dummy's
//! coordinate. The pool's threads persist, so their thread-local scratch (the
//! path search workspaces) stays allocated between frames.

use rayon::prelude::*;

/// `work` over every item, results in item order. Workers steal items from
/// each other, so slower cores (efficiency cores, a busy host) take fewer.
/// A worker's panic resumes on the caller.
pub(crate) fn map<T: Sync, R: Send>(items: &[T], work: impl Fn(&T) -> R + Sync) -> Vec<R> {
    // A shared reference to a `Sync` closure is `Send`.
    items.par_iter().map(&work).collect()
}

/// [`map`] with scratch: each worker takes its own clone of `init` and keeps
/// it across the items it takes, so `work` may cache in it answers that do
/// not depend on the item.
pub(crate) fn map_with<T: Sync, S: Clone + Send, R: Send>(
    items: &[T],
    init: S,
    work: impl Fn(&mut S, &T) -> R + Sync,
) -> Vec<R> {
    items
        .par_iter()
        .map_with(init, |scratch, item| work(scratch, item))
        .collect()
}

/// The indices below `len` that `keep` selects, in ascending order.
pub(crate) fn select_indices(len: usize, keep: impl Fn(usize) -> bool + Sync) -> Vec<usize> {
    (0..len)
        .into_par_iter()
        .filter(|&index| keep(index))
        .collect()
}

/// `pick`'s answers for the indices below `len` it answers, in ascending
/// index order.
pub(crate) fn filter_map_indices<R: Send>(
    len: usize,
    pick: impl Fn(usize) -> Option<R> + Sync,
) -> Vec<R> {
    // A shared reference to a `Sync` closure is `Send`.
    (0..len).into_par_iter().filter_map(&pick).collect()
}

/// `work` on every item in place.
pub(crate) fn for_each_mut<T: Send>(items: &mut [T], work: impl Fn(&mut T) + Sync) {
    items.par_iter_mut().for_each(&work);
}

/// Whether `test` holds for any index below `len`.
pub(crate) fn any_index(len: usize, test: impl Fn(usize) -> bool + Sync) -> bool {
    (0..len).into_par_iter().any(&test)
}

/// Run `pooled` on the worker pool while `here` runs on this thread, and
/// return once both are done. Only `pooled` crosses threads, so `here` may
/// hold this thread's `&mut` borrows and read its thread-locals. A panic in
/// either resumes on the caller.
pub(crate) fn alongside(pooled: impl FnOnce() + Send, here: impl FnOnce()) {
    rayon::in_place_scope(|scope| {
        scope.spawn(move |_| pooled());
        here();
    });
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
        let mut doubled = items.clone();
        super::for_each_mut(&mut doubled, |item| *item *= 2);
        assert_eq!(
            doubled,
            items.iter().map(|item| item * 2).collect::<Vec<_>>()
        );
        assert!(super::any_index(1000, |index| index == 999));
        assert!(!super::any_index(1000, |index| index >= 1000));
    }

    #[test]
    fn alongside_runs_here_on_the_caller_and_waits_for_the_pooled_work() {
        let items: Vec<u64> = (0..1000).collect();
        let mut pooled_sum = 0;
        let mut here_thread = None;
        super::alongside(
            || pooled_sum = super::map(&items, |item| item * 3).iter().sum(),
            || here_thread = Some(std::thread::current().id()),
        );
        assert_eq!(
            (here_thread, pooled_sum),
            (Some(std::thread::current().id()), 3 * 499_500)
        );
        // On a one-thread pool's own worker, the caller runs the pooled work.
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(1)
            .build()
            .expect("a worker pool");
        let mut ran = false;
        pool.install(|| super::alongside(|| ran = true, || ()));
        assert!(ran);
    }
}
