//! Order-preserving work over rayon's persistent worker pool. Each result
//! depends on its item alone, so the output never depends on the thread
//! count or on scheduling. Work run [`alongside`] the caller and the caller
//! must only read what they share: the borrow checker rules out a shared
//! `&mut`, not interior mutability such as the shared cell dummy's
//! coordinate. The pool's threads persist, so their thread-local scratch (the
//! path search workspaces) stays allocated between frames.
//!
//! The simulation runs on a thread outside the pool, which rayon would put
//! to sleep until the workers had answered. Called from such a thread, a
//! helper takes items there too ([`side_by_side`]): the caller and the
//! workers it enlists each pull the next indices from a shared counter until
//! none is left, and the caller then waits only for the items still under
//! way. Answers go back in index order, so nothing depends on which thread
//! took an item or on how many threads there were. An item may therefore run
//! on the calling thread: it must not need a thread-local the caller holds
//! borrowed. Called on a worker, a helper leaves the splitting to rayon,
//! whose waiting workers take other work already.

use std::cell::Cell;
use std::ops::Range;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

use rayon::prelude::*;

/// `work` over every item, results in item order. Threads take the items a
/// few at a time, so slower cores (efficiency cores, a busy host) take fewer.
/// A worker's panic resumes on the caller.
pub fn map<T: Sync, R: Send>(items: &[T], work: impl Fn(&T) -> R + Sync) -> Vec<R> {
    if caller_takes() {
        return pulled(Takes::chunked(items.len()), (), |(), index| {
            Some(work(&items[index]))
        });
    }
    // A shared reference to a `Sync` closure is `Send`.
    items.par_iter().map(&work).collect()
}

/// [`map`] that starts the items in slice order, each on the next free
/// thread, and [`filter_map_indices`] below `len`, as one body of work
/// answered as each would answer alone: every thread starts on the items
/// and goes on to the indices once no item is left. Starting the costliest
/// items first keeps the slowest from running alone at the end, the indices
/// do not wait for it, and a thread whose items were short is not idle while
/// another's is long. `work` also gets the item's index.
pub(crate) fn map_in_order_then_filter_map_indices<T: Sync, A: Send, B: Send>(
    items: &[T],
    work: impl Fn(usize, &T) -> A + Sync,
    len: usize,
    pick: impl Fn(usize) -> Option<B> + Sync,
) -> (Vec<A>, Vec<B>) {
    enum Answer<A, B> {
        Item(A),
        Index(B),
    }
    let takes = Takes::one_by_one(items.len()).then_chunked(len);
    let answers = pulled(takes, (), |(), index| {
        match index.checked_sub(items.len()) {
            None => Some(Answer::Item(work(index, &items[index]))),
            Some(index) => pick(index).map(Answer::Index),
        }
    });
    let mut of_items = Vec::with_capacity(items.len());
    let mut of_indices = Vec::with_capacity(answers.len() - items.len());
    for answer in answers {
        match answer {
            Answer::Item(answer) => of_items.push(answer),
            Answer::Index(answer) => of_indices.push(answer),
        }
    }
    (of_items, of_indices)
}

/// [`map`] with scratch: each thread takes its own clone of `init` and keeps
/// it across the items it takes, so `work` may cache in it answers that do
/// not depend on the item.
pub(crate) fn map_with<T: Sync, S: Clone + Send, R: Send>(
    items: &[T],
    init: S,
    work: impl Fn(&mut S, &T) -> R + Sync,
) -> Vec<R> {
    if caller_takes() {
        return pulled(Takes::chunked(items.len()), init, |scratch, index| {
            Some(work(scratch, &items[index]))
        });
    }
    items
        .par_iter()
        .map_with(init, |scratch, item| work(scratch, item))
        .collect()
}

/// The indices below `len` that `keep` selects, in ascending order.
pub(crate) fn select_indices(len: usize, keep: impl Fn(usize) -> bool + Sync) -> Vec<usize> {
    filter_map_indices(len, |index| keep(index).then_some(index))
}

/// `pick`'s answers for the indices below `len` it answers, in ascending
/// index order.
pub(crate) fn filter_map_indices<R: Send>(
    len: usize,
    pick: impl Fn(usize) -> Option<R> + Sync,
) -> Vec<R> {
    if caller_takes() {
        return pulled(Takes::chunked(len), (), |(), index| pick(index));
    }
    // A shared reference to a `Sync` closure is `Send`.
    (0..len).into_par_iter().filter_map(&pick).collect()
}

/// `work` on every item in place.
pub(crate) fn for_each_mut<T: Send>(items: &mut [T], work: impl Fn(&mut T) + Sync) {
    if !caller_takes() {
        return items.par_iter_mut().for_each(&work);
    }
    let takes = Takes::chunked(items.len());
    // A counter can hand out indices, not `&mut` items: each taker takes the
    // next chunk under the lock and works on it outside.
    let chunks = Mutex::new(items.chunks_mut(takes.step));
    let chunks = || chunks.lock().expect("no taker panics under the lock");
    side_by_side(
        takes.count(),
        (),
        || chunks().len() != 0,
        |()| {
            loop {
                let Some(chunk) = chunks().next() else {
                    break;
                };
                chunk.iter_mut().for_each(&work);
            }
        },
    );
}

/// Whether `test` holds for any index below `len`.
pub(crate) fn any_index(len: usize, test: impl Fn(usize) -> bool + Sync) -> bool {
    if !caller_takes() {
        return (0..len).into_par_iter().any(&test);
    }
    let takes = Takes::chunked(len);
    let next = AtomicUsize::new(0);
    let found = AtomicBool::new(false);
    side_by_side(
        takes.count(),
        (),
        || !found.load(Ordering::Relaxed) && takes.left_from(next.load(Ordering::Relaxed)),
        |()| {
            while !found.load(Ordering::Relaxed) {
                let Some(range) = takes.range(next.fetch_add(1, Ordering::Relaxed)) else {
                    break;
                };
                if range.into_iter().any(&test) {
                    found.store(true, Ordering::Relaxed);
                }
            }
        },
    );
    found.into_inner()
}

/// Run `pooled` on the worker pool while `here` runs on this thread, and
/// return once both are done. Only `pooled` crosses threads, so `here` may
/// hold this thread's `&mut` borrows and read its thread-locals. A panic in
/// either resumes on the caller.
#[cfg(not(target_arch = "wasm32"))]
pub fn alongside(pooled: impl FnOnce() + Send, here: impl FnOnce()) {
    rayon::in_place_scope(|scope| {
        scope.spawn(move |_| pooled());
        here();
    });
}

/// The browser's main thread traps if it waits on a contended lock, so it runs
/// no engine work while the pool does: `pooled` (whose own parallel work runs
/// on the pool) finishes before `here` starts. The two share only reads, so
/// their order changes no result.
#[cfg(target_arch = "wasm32")]
pub fn alongside(pooled: impl FnOnce() + Send, here: impl FnOnce()) {
    pooled();
    here();
}

/// Drop `value` on the worker pool, so the caller does not wait for it to be
/// freed. The browser drops it here: its pool may not run.
pub fn drop_on_pool<T: Send + 'static>(value: T) {
    #[cfg(not(target_arch = "wasm32"))]
    rayon::spawn(move || drop(value));
    #[cfg(target_arch = "wasm32")]
    drop(value);
}

/// rayon's global pool as the game starts it: each worker has the game's
/// stack and a name. A helper's item runs on the calling thread or on a
/// worker ([`side_by_side`]), so with rayon's default 2 MiB a deep item that
/// fits the simulation thread could overflow a worker. A crash report then
/// names the worker.
#[cfg(not(target_arch = "wasm32"))]
pub fn worker_pool() -> rayon::ThreadPoolBuilder {
    rayon::ThreadPoolBuilder::new()
        .stack_size(crate::util::GAME_STACK_BYTES)
        .thread_name(|index| format!("worker {index}"))
}

/// Start the global pool from [`worker_pool`], before anything uses it. A
/// second call does nothing; a pool something else already started stays,
/// with a warning.
#[cfg(not(target_arch = "wasm32"))]
pub fn start_worker_pool() {
    static STARTED: std::sync::Once = std::sync::Once::new();
    STARTED.call_once(|| {
        if let Err(error) = worker_pool().build_global() {
            log::warn!("The worker pool keeps rayon's default stack: {error}");
        }
    });
}

/// Run a fixture on the pool's only worker so deferred drops stay queued until it yields.
#[cfg(all(not(target_arch = "wasm32"), any(test, feature = "test-support")))]
pub fn on_single_worker_for_test(work: impl FnOnce() + Send) {
    rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .build()
        .expect("a worker pool")
        .install(work);
}

/// Whether the calling thread takes items itself ([`side_by_side`]): it is
/// outside the pool, where waiting for the workers is sleeping. The
/// browser's threaded build always leaves the items to the pool, as its main
/// thread runs no engine work while the pool does (see [`alongside`]).
fn caller_takes() -> bool {
    cfg!(not(target_arch = "wasm32")) && rayon::current_thread_index().is_none()
}

/// How a call's indices are handed out: the first `singles` one at a time,
/// in order, and the rest `step` at a time.
#[derive(Clone, Copy)]
struct Takes {
    len: usize,
    singles: usize,
    step: usize,
}

impl Takes {
    /// `len` indices one at a time, for items that are costly or must start
    /// in order.
    fn one_by_one(len: usize) -> Self {
        Self {
            len,
            singles: len,
            step: 1,
        }
    }

    /// `len` indices a few at a time: enough that taking them costs little
    /// beside the work, few enough that a faster thread takes what a slower
    /// one leaves. A short list goes one at a time, since its items may be
    /// costly.
    fn chunked(len: usize) -> Self {
        Self::one_by_one(0).then_chunked(len)
    }

    /// These takes, then `len` more indices a few at a time.
    fn then_chunked(self, len: usize) -> Self {
        Self {
            len: self.len + len,
            singles: self.len,
            step: (len / ((rayon::current_num_threads() + 1) * 16)).max(1),
        }
    }

    /// How many takes hand out every index.
    fn count(self) -> usize {
        self.singles + (self.len - self.singles).div_ceil(self.step)
    }

    /// Whether take `take` still hands out indices.
    fn left_from(self, take: usize) -> bool {
        take < self.count()
    }

    /// The indices of take `take`; `None` once all are handed out.
    fn range(self, take: usize) -> Option<Range<usize>> {
        if take < self.singles {
            return Some(take..take + 1);
        }
        let start = (take - self.singles)
            .checked_mul(self.step)?
            .checked_add(self.singles)?;
        (start < self.len).then(|| start..self.len.min(start + self.step))
    }
}

/// What one thread took of a call: its answers in the order it made them,
/// and for each take that answered, its first index and how many answers.
struct Taken<R> {
    answers: Vec<R>,
    takes: Vec<(usize, usize)>,
}

/// `work`'s answers for the indices of `takes` it answers, in index order,
/// each thread with its own clone of `init`. Off the pool the caller takes
/// indices beside the workers it enlists ([`side_by_side`]); on it, a puller
/// per worker does, where a busy worker's puller starts late and finds none
/// left.
fn pulled<S: Clone + Send, R: Send>(
    takes: Takes,
    init: S,
    work: impl Fn(&mut S, usize) -> Option<R> + Sync,
) -> Vec<R> {
    let next = AtomicUsize::new(0);
    let pull = |scratch: &mut S| {
        let mut taken = Taken {
            answers: Vec::new(),
            takes: Vec::new(),
        };
        while let Some(range) = takes.range(next.fetch_add(1, Ordering::Relaxed)) {
            let (start, before) = (range.start, taken.answers.len());
            taken
                .answers
                .extend(range.filter_map(|index| work(scratch, index)));
            if taken.answers.len() > before {
                taken.takes.push((start, taken.answers.len() - before));
            }
        }
        taken
    };
    let taken: Vec<Taken<R>> = if caller_takes() {
        side_by_side(
            takes.count(),
            init,
            || takes.left_from(next.load(Ordering::Relaxed)),
            pull,
        )
    } else {
        (0..rayon::current_num_threads().min(takes.count()))
            .into_par_iter()
            .with_max_len(1)
            .map_with(init, |scratch, _| pull(scratch))
            .collect()
    };
    in_index_order(taken)
}

/// Every thread's answers as one list in index order. A thread takes
/// ascending indices, so each one's answers are in order already.
fn in_index_order<R>(mut taken: Vec<Taken<R>>) -> Vec<R> {
    taken.retain(|taken| !taken.answers.is_empty());
    if taken.len() <= 1 {
        return taken.pop().map_or_else(Vec::new, |taken| taken.answers);
    }
    let mut order: Vec<(usize, usize, usize)> = taken
        .iter()
        .enumerate()
        .flat_map(|(taker, taken)| {
            let takes = taken.takes.iter();
            takes.map(move |&(start, count)| (start, taker, count))
        })
        .collect();
    order.sort_unstable_by_key(|&(start, ..)| start);
    let mut answers: Vec<_> = taken
        .into_iter()
        .map(|taken| taken.answers.into_iter())
        .collect();
    let mut ordered = Vec::with_capacity(answers.iter().map(ExactSizeIterator::len).sum());
    for (_, taker, count) in order {
        ordered.extend(answers[taker].by_ref().take(count));
    }
    ordered
}

thread_local! {
    /// Whether this thread, outside the pool, is pulling its own share of a
    /// call ([`side_by_side`]).
    static PULLING: Cell<bool> = const { Cell::new(false) };
}

/// `pull` on this thread and on the workers it enlists, each with its own
/// clone of `scratch`, until every one has returned: their answers, this
/// thread's first. `takes` is how many takes the call has; no more threads
/// pull than that, nor than [`most_helpers`] and this one.
///
/// This thread sends for one worker and pulls at once. Each worker that
/// arrives sends for two more while `work_left`, and pulls too, so the pool
/// wakes as fast as rayon's own splitting wakes it, and no faster than the
/// work lasts: a list this thread finishes before help arrives wakes one
/// worker. A job no late worker claims is run by the worker that sent for
/// it, and finds nothing left. So this thread waits at the end for the items
/// under way, or for its first helper to arrive.
///
/// A call from inside this thread's own share pulls alone: the workers are
/// busy with the outer call's items, and rayon's scope would have this
/// thread sleep until one came free to find nothing left.
fn side_by_side<S: Clone + Send, T: Send>(
    takes: usize,
    mut scratch: S,
    work_left: impl Fn() -> bool + Sync,
    pull: impl Fn(&mut S) -> T + Sync,
) -> Vec<T> {
    // A worker's share of pooled work never reaches a test's RNG trace, so
    // this thread's share stays out of it too.
    #[cfg(any(test, feature = "test-support"))]
    let _untraced = crate::sim::rng::untraced_draws();
    let helpers = if PULLING.get() {
        0
    } else {
        most_helpers().min(takes.saturating_sub(1))
    };
    if helpers == 0 {
        return vec![pull(&mut scratch)];
    }
    struct Pulling;
    impl Drop for Pulling {
        fn drop(&mut self) {
            PULLING.set(false);
        }
    }
    let mut theirs: Vec<Option<T>> = std::iter::repeat_with(|| None).take(helpers).collect();
    let mut mine = None;
    rayon::in_place_scope(|scope| {
        enlist(scope, &mut theirs, &scratch, &work_left, &pull);
        PULLING.set(true);
        let _pulling = Pulling;
        mine = Some(pull(&mut scratch));
    });
    mine.into_iter()
        .chain(theirs.into_iter().flatten())
        .collect()
}

/// The most workers a call off the pool enlists: all of them, but with the
/// caller no more threads than the machine has cores, so that a pool as
/// large as the machine and the caller do not take turns on a core. At
/// least one, so the pool is always asked.
fn most_helpers() -> usize {
    static CORES: OnceLock<usize> = OnceLock::new();
    let cores =
        *CORES.get_or_init(|| std::thread::available_parallelism().map_or(usize::MAX, usize::from));
    rayon::current_num_threads().min(cores.saturating_sub(1).max(1))
}

/// Send for a worker to `pull` into the first of `slots` with a clone of
/// `scratch`. When it arrives it first sends for two more, each with half of
/// the other slots, if `work_left`.
fn enlist<'scope, S, T, L, P>(
    scope: &rayon::Scope<'scope>,
    slots: &'scope mut [Option<T>],
    scratch: &S,
    work_left: &'scope L,
    pull: &'scope P,
) where
    S: Clone + Send + 'scope,
    T: Send,
    L: Fn() -> bool + Sync,
    P: Fn(&mut S) -> T + Sync,
{
    let Some((slot, rest)) = slots.split_first_mut() else {
        return;
    };
    let mut scratch = scratch.clone();
    scope.spawn(move |scope| {
        if work_left() {
            let (left, right) = rest.split_at_mut(rest.len().div_ceil(2));
            enlist(scope, left, &scratch, work_left, pull);
            enlist(scope, right, &scratch, work_left, pull);
        }
        *slot = Some(pull(&mut scratch));
    });
}

#[cfg(all(test, tests_sim_p_s))]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};

    /// Recursion whose frames together take at least `depth` half-KiB.
    #[cfg(not(target_arch = "wasm32"))]
    fn nest(depth: usize) -> u8 {
        let frame = std::hint::black_box([depth as u8; 512]);
        if depth == 0 {
            return frame[0];
        }
        frame[511].wrapping_add(nest(depth - 1))
    }

    /// A worker of the game's pool is named and holds an item of over 3 MiB,
    /// more than rayon's default 2 MiB stack.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_game_pool_worker_has_the_game_stack_and_a_name() {
        let pool = super::worker_pool()
            .num_threads(1)
            .build()
            .expect("a worker pool");
        let name = pool.install(|| {
            std::hint::black_box(nest(6 * 1024));
            std::thread::current().name().map(str::to_owned)
        });
        assert_eq!(name.as_deref(), Some("worker 0"));
    }

    /// Every helper answers as a serial loop does, from an empty list to one
    /// handed out many indices at a time.
    #[test]
    fn results_keep_item_order() {
        for len in [0usize, 1, 2, 1000, 20_011] {
            let items: Vec<u64> = (0..len as u64).collect();
            let squares: Vec<u64> = items.iter().map(|item| item * item).collect();
            let picked: Vec<usize> = (0..len).filter(|index| index % 7 == 3).collect();
            assert_eq!(super::map(&items, |item| item * item), squares, "{len}");
            assert_eq!(
                super::map_with(&items, 2, |power, item| item.pow(*power)),
                squares
            );
            assert_eq!(super::select_indices(len, |index| index % 7 == 3), picked);
            assert_eq!(
                super::filter_map_indices(len, |index| (index % 7 == 3).then_some(index)),
                picked
            );
            let mut doubled = items.clone();
            super::for_each_mut(&mut doubled, |item| *item *= 2);
            assert!(
                doubled
                    .iter()
                    .copied()
                    .eq(items.iter().map(|item| item * 2))
            );
            assert_eq!(super::any_index(len, |index| index + 1 == len), len > 0);
            assert!(!super::any_index(len, |index| index >= len));
            let (in_order, joined) = super::map_in_order_then_filter_map_indices(
                &items,
                |index, item| item * item + index as u64,
                len,
                |index| (index % 7 == 3).then_some(index),
            );
            assert!(
                in_order
                    .into_iter()
                    .eq(items.iter().map(|item| item * item + item))
            );
            assert_eq!(joined, picked);
        }
    }

    /// Off the pool the calling thread takes items itself. Every other
    /// thread holds its first item until the caller has taken one, so a
    /// caller that only waited would never be seen.
    #[test]
    fn the_calling_thread_takes_items_off_the_pool() {
        let caller = std::thread::current().id();
        let (took, gave_up) = (AtomicBool::new(false), AtomicBool::new(false));
        let item = || {
            if std::thread::current().id() == caller {
                return took.store(true, Ordering::Relaxed);
            }
            let waiting = Instant::now();
            while !took.load(Ordering::Relaxed) && !gave_up.load(Ordering::Relaxed) {
                if waiting.elapsed() > Duration::from_secs(5) {
                    gave_up.store(true, Ordering::Relaxed);
                }
                std::thread::yield_now();
            }
        };
        let items = [0u8; 64];
        let check = |helper: &str, call: &dyn Fn()| {
            took.store(false, Ordering::Relaxed);
            call();
            assert!(took.load(Ordering::Relaxed), "{helper}");
        };
        check("map", &|| drop(super::map(&items, |_| item())));
        check("map_with", &|| {
            drop(super::map_with(&items, (), |(), _| item()));
        });
        check("filter_map_indices", &|| {
            drop(super::filter_map_indices(64, |_| Some(item())));
        });
        check("for_each_mut", &|| {
            super::for_each_mut(&mut [0u8; 64], |_| item());
        });
        check("any_index", &|| {
            let any = super::any_index(64, |_| {
                item();
                false
            });
            assert!(!any);
        });
        check("map_in_order_then_filter_map_indices", &|| {
            drop(super::map_in_order_then_filter_map_indices(
                &items[..8],
                |_, _| item(),
                64,
                |_| Some(item()),
            ));
        });
    }

    /// A joined call answers its indices while an item is still under way:
    /// its one item waits for an index to be answered.
    #[test]
    fn joined_indices_do_not_wait_for_the_slowest_item() {
        let answered = AtomicBool::new(false);
        let (waited, indices) = super::map_in_order_then_filter_map_indices(
            &[()],
            |_, ()| {
                let waiting = Instant::now();
                while !answered.load(Ordering::Relaxed)
                    && waiting.elapsed() < Duration::from_secs(5)
                {
                    std::thread::yield_now();
                }
                answered.load(Ordering::Relaxed)
            },
            1000,
            |index| {
                answered.store(true, Ordering::Relaxed);
                (index % 2 == 0).then_some(index)
            },
        );
        assert_eq!(waited, [true]);
        assert!(indices.into_iter().eq((0..1000).step_by(2)));
    }

    /// A call made inside another's item answers as it does alone, whether
    /// the item runs on the calling thread, which then takes the inner
    /// call's items alone, or on a worker, where rayon splits them.
    #[test]
    fn nested_calls_answer_as_serial_ones() {
        let rows: Vec<u64> = (0..300).collect();
        let sums = super::map(&rows, |&row| {
            let inner: Vec<u64> = (0..row).collect();
            let tripled = super::map_with(&inner, 3, |times, item| item * *times);
            tripled.iter().sum::<u64>()
        });
        let serial = rows.iter().map(|row| 3 * row * row.saturating_sub(1) / 2);
        assert!(sums.into_iter().eq(serial));
    }

    #[test]
    #[should_panic(expected = "item 7")]
    fn an_items_panic_resumes_on_the_caller() {
        super::filter_map_indices(64, |index| {
            assert_ne!(index, 7, "item 7");
            Some(index)
        });
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
