//! Single-threaded browser execution of the existing independent work items.
pub(crate) fn map<T: Sync, R: Send>(items: &[T], work: impl Fn(&T) -> R + Sync) -> Vec<R> {
    items.iter().map(work).collect()
}
pub(crate) fn map_in_order<T: Sync, B, R: Send>(
    items: &[T],
    before: impl Fn() -> B + Sync,
    work: impl Fn(&T, B) -> R + Sync,
) -> Vec<R> {
    items.iter().map(|item| work(item, before())).collect()
}
pub(crate) fn map_with<T: Sync, S: Clone + Send, R: Send>(
    items: &[T],
    mut init: S,
    work: impl Fn(&mut S, &T) -> R + Sync,
) -> Vec<R> {
    items.iter().map(|item| work(&mut init, item)).collect()
}
pub(crate) fn select_indices(len: usize, keep: impl Fn(usize) -> bool + Sync) -> Vec<usize> {
    (0..len).filter(|&i| keep(i)).collect()
}
pub(crate) fn filter_map_indices<R: Send>(
    len: usize,
    pick: impl Fn(usize) -> Option<R> + Sync,
) -> Vec<R> {
    (0..len).filter_map(pick).collect()
}
pub(crate) fn for_each_mut<T: Send>(items: &mut [T], work: impl Fn(&mut T) + Sync) {
    items.iter_mut().for_each(work);
}
pub(crate) fn any_index(len: usize, test: impl Fn(usize) -> bool + Sync) -> bool {
    (0..len).any(test)
}
pub(crate) fn alongside(pooled: impl FnOnce() + Send, here: impl FnOnce()) {
    pooled();
    here();
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::sim::parallel as native;

    #[test]
    fn serial_work_preserves_native_order_and_completion() {
        // Independent work must agree regardless of scheduling or worker count.
        let input = [7, 1, 9, 0, 4];
        assert_eq!(map(&input, |x| x * x), native::map(&input, |x| x * x));
        assert_eq!(
            map_in_order(&input, || 3, |x, n| x + n),
            native::map_in_order(&input, || 3, |x, n| x + n)
        );
        let work = |scratch: &mut Vec<i32>, x: &i32| {
            scratch.clear();
            scratch.push(x * x);
            scratch[0]
        };
        assert_eq!(
            map_with(&input, Vec::new(), work),
            native::map_with(&input, Vec::new(), work)
        );
        assert_eq!(
            select_indices(17, |x| x % 3 == 0),
            native::select_indices(17, |x| x % 3 == 0)
        );
        assert_eq!(
            filter_map_indices(17, |x| (x % 3 == 0).then_some(x * 2)),
            native::filter_map_indices(17, |x| (x % 3 == 0).then_some(x * 2))
        );
        let (mut serial, mut pooled) = (input, input);
        for_each_mut(&mut serial, |x| *x += 1);
        native::for_each_mut(&mut pooled, |x| *x += 1);
        assert_eq!(serial, pooled);
        assert_eq!(
            any_index(17, |x| x == 16),
            native::any_index(17, |x| x == 16)
        );
        assert!(!any_index(0, |_| panic!("empty work must not execute")));
        let (mut first, mut second) = (false, false);
        alongside(|| first = true, || second = true);
        assert!(first && second);
    }
}
