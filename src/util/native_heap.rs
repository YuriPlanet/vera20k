//! gamemd's binary min-heap, the one port every caller uses.
//!
//! gamemd compiles the same heap into each user: `FloatMinHeap::SiftDown @
//! 0x005AD870` (the random map's floods, `TiberiumClass`'s growth and spread
//! processors, the Veinhole Monster), `MinHeap__SiftDown @ 0x0042DCA0`
//! (`Zone_precheck`, from `0x0042C4C0`), the cell A*'s inline insert
//! `0x0042A061..0x0042A122` and pop `0x0042A2BB..0x0042A3B1`, and the inline
//! inserts at `0x0042C401`/`0x0042C69B` (precheck), `0x0059C23C` (blob),
//! `0x005A0BA2`/`0x005A0F7B` (meander) and `0x00723476` (Tiberium rebuild).
//!
//! Every copy compares strictly: an insert climbs only past a parent that
//! costs more, and a sift moves down only to a child that costs less, the
//! left one first, so equal costs keep their array order. A `BinaryHeap` with
//! a tie-break does not reproduce that order. An insert that would bring the
//! count to the heap's size word is dropped (`slot = count + 1; CMP slot,size;
//! JNC`). The caller supplies the cost comparison on every call.

/// A 0-based array heap; native slot `i` (1-based) is index `i - 1` here.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct NativeHeap<T> {
    slots: Vec<T>,
    /// The size word (`+0x04`).
    size: usize,
}

impl<T> NativeHeap<T> {
    /// An empty heap with the size word `size`.
    pub(crate) const fn new(size: usize) -> Self {
        Self {
            slots: Vec::new(),
            size,
        }
    }

    /// A heap holding `slots` as native left them, slot 1 first.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn from_native_slots(slots: Vec<T>, size: usize) -> Self {
        Self { slots, size }
    }

    pub(crate) fn size(&self) -> usize {
        self.size
    }

    /// The count (`+0x00`).
    pub(crate) fn len(&self) -> usize {
        self.slots.len()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// The slots in native order, slot 1 first.
    pub(crate) fn as_slice(&self) -> &[T] {
        &self.slots
    }

    /// The root, which the next pop returns.
    pub(crate) fn first(&self) -> Option<&T> {
        self.slots.first()
    }

    pub(crate) fn clear(&mut self) {
        self.slots.clear();
    }

    /// Empty the heap and give it the size word `size`, keeping the
    /// allocation.
    pub(crate) fn reset(&mut self, size: usize) {
        self.slots.clear();
        self.size = size;
    }
}

impl<T: Copy> NativeHeap<T> {
    /// Insert `item`, climbing past each parent that `less` puts after it.
    /// Returns false, inserting nothing, when the heap is full.
    pub(crate) fn push(&mut self, item: T, less: impl Fn(&T, &T) -> bool) -> bool {
        if self.slots.len() + 1 >= self.size {
            return false;
        }
        let mut position = self.slots.len();
        self.slots.push(item);
        while position != 0 {
            let parent = (position - 1) / 2;
            if !less(&item, &self.slots[parent]) {
                break;
            }
            self.slots[position] = self.slots[parent];
            position = parent;
        }
        self.slots[position] = item;
        true
    }

    /// Remove the root, moving the last item to the root and sifting it down.
    pub(crate) fn pop(&mut self, less: impl Fn(&T, &T) -> bool) -> Option<T> {
        let first = *self.slots.first()?;
        let last = self.slots.pop().expect("nonempty heap");
        if !self.slots.is_empty() {
            self.slots[0] = last;
            self.sift_down(&less);
        }
        Some(first)
    }

    /// Put `item` at the root and sift it down, returning the old root. The
    /// A* takes its next node this way when the held child costs no less
    /// than the root (`0x0042A1E8`).
    pub(crate) fn replace_first(&mut self, item: T, less: impl Fn(&T, &T) -> bool) -> T {
        let first = std::mem::replace(&mut self.slots[0], item);
        self.sift_down(&less);
        first
    }

    /// Sift the root down: at each level the item moves to the cheaper child
    /// if that child costs less than it, the left one when they tie. The
    /// item is held aside and written once where it stops.
    ///
    /// Native tests the left child against the item, then the right against
    /// the cheaper of the two. Choosing the cheaper child first (the right
    /// only when it costs strictly less) and testing it against the item
    /// moves to the same child: the chosen child costs no more than the
    /// other, so it costs less than the item whenever native moves.
    fn sift_down(&mut self, less: &impl Fn(&T, &T) -> bool) {
        let Some(&item) = self.slots.first() else {
            return;
        };
        let len = self.slots.len();
        let mut position = 0;
        loop {
            let left = position * 2 + 1;
            if left >= len {
                break;
            }
            let right = left + 1;
            let child = if right < len && less(&self.slots[right], &self.slots[left]) {
                right
            } else {
                left
            };
            if !less(&self.slots[child], &item) {
                break;
            }
            self.slots[position] = self.slots[child];
            position = child;
        }
        self.slots[position] = item;
    }
}

#[cfg(test)]
mod tests {
    use super::NativeHeap;

    /// (cost, arrival) pairs compared by cost alone.
    fn less(a: &(u32, u32), b: &(u32, u32)) -> bool {
        a.0 < b.0
    }

    #[test]
    fn equal_costs_keep_the_native_array_order() {
        let mut heap = NativeHeap::new(usize::MAX);
        for (arrival, cost) in [5, 3, 3, 7, 3, 1, 3].into_iter().enumerate() {
            heap.push((cost, arrival as u32), less);
        }
        let mut order = Vec::new();
        while let Some(item) = heap.pop(less) {
            order.push(item);
        }
        // Not the arrival order among the threes, nor any tie-break: the
        // strict climbs and sifts leave them as the array has them.
        assert_eq!(
            order,
            [(1, 5), (3, 6), (3, 2), (3, 4), (3, 1), (5, 0), (7, 3)]
        );
    }

    #[test]
    fn replace_first_returns_the_root_and_sifts_the_new_item() {
        let mut heap = NativeHeap::new(usize::MAX);
        for (arrival, cost) in [2, 4, 4, 6].into_iter().enumerate() {
            heap.push((cost, arrival as u32), less);
        }
        assert_eq!(heap.replace_first((4, 9), less), (2, 0));
        // The new root ties with its left child and stays put.
        assert_eq!(heap.first(), Some(&(4, 9)));
        assert_eq!(heap.replace_first((5, 8), less), (4, 9));
        assert_eq!(heap.first(), Some(&(4, 1)));
    }

    #[test]
    fn a_full_heap_drops_the_insert() {
        // Size 3 holds two items: the third insert would bring the count to
        // the size, even though it is the cheapest.
        let mut heap = NativeHeap::new(3);
        for (arrival, cost) in [2, 4, 1].into_iter().enumerate() {
            assert_eq!(heap.push((cost, arrival as u32), less), arrival < 2);
        }
        assert_eq!(heap.pop(less), Some((2, 0)));
        assert_eq!(heap.pop(less), Some((4, 1)));
        assert_eq!(heap.pop(less), None);
        // Size 0 holds nothing.
        assert!(!NativeHeap::new(0).push((0, 0), less));
    }
}
