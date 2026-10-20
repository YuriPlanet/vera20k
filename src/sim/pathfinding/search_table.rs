//! Search scratch that forgets every entry at once.

/// Per-index values that one search writes and reads, reused across searches.
///
/// An entry belongs to the running search only while its bit is set, and
/// [`Self::begin`] clears the words the last search wrote, so starting a
/// search costs as little as the last one wrote and [`Self::contains`],
/// which A* asks of nearly every candidate, reads one word that stays in the
/// fastest caches. Nothing an earlier search wrote counts. Pure scratch: never
/// saved or hashed.
#[derive(Debug)]
pub(super) struct SearchTable<T> {
    /// One bit per index (bit `index % 64` of word `index / 64`), set when
    /// the running search wrote it.
    written: Vec<u64>,
    /// The words of `written` the running search set bits in (empty when
    /// `start > end`); every other word is zero.
    dirty: std::ops::Range<usize>,
    values: Vec<T>,
}

impl<T> Default for SearchTable<T> {
    fn default() -> Self {
        Self {
            written: Vec::new(),
            dirty: usize::MAX..0,
            values: Vec::new(),
        }
    }
}

impl<T: Copy> SearchTable<T> {
    /// Start a search over indices `0..len`, forgetting every entry. Slots
    /// that grow the table hold `blank`, which no search reads.
    pub(super) fn begin(&mut self, len: usize, blank: T) {
        if self.values.len() < len {
            self.values.resize(len, blank);
        }
        let end = self.dirty.end.min(self.written.len());
        self.written[self.dirty.start.min(end)..end].fill(0);
        self.dirty = usize::MAX..0;
        self.written.resize(len.div_ceil(64), 0);
    }

    pub(super) fn contains(&self, index: usize) -> bool {
        self.written[index / 64] >> (index % 64) & 1 != 0
    }

    /// The running search's entry at `index`, if it wrote one.
    pub(super) fn get(&self, index: usize) -> Option<T> {
        self.contains(index).then(|| self.values[index])
    }

    pub(super) fn set(&mut self, index: usize, value: T) {
        let word = index / 64;
        self.written[word] |= 1 << (index % 64);
        self.dirty = self.dirty.start.min(word)..self.dirty.end.max(word + 1);
        self.values[index] = value;
    }

    /// A table whose running search wrote every one of `values`.
    #[cfg(any(test, feature = "test-support"))]
    pub(super) fn with_all(values: Vec<T>) -> Self {
        let words = values.len().div_ceil(64);
        Self {
            written: vec![u64::MAX; words],
            dirty: 0..words,
            values,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SearchTable;

    #[test]
    fn entries_in_neighbouring_words_stay_apart() {
        let mut table = SearchTable::default();
        table.begin(200, 0u16);
        for index in [0, 63, 64, 127, 199] {
            table.set(index, index as u16 + 1);
        }
        for index in 0..200 {
            let written = [0, 63, 64, 127, 199].contains(&index);
            assert_eq!(
                table.get(index),
                written.then_some(index as u16 + 1),
                "{index}"
            );
        }
        // A shorter search forgets every word, the ones past its end too.
        table.begin(64, 0);
        assert!((0..64).all(|index| !table.contains(index)));
        table.begin(200, 0);
        assert!((0..200).all(|index| !table.contains(index)));
    }

    /// A search clears only the words the last one wrote, and a table built
    /// whole forgets all of them.
    #[test]
    fn a_search_clears_what_the_last_one_wrote() {
        let mut table = SearchTable::default();
        table.begin(1000, 0u8);
        table.set(700, 1);
        table.set(130, 2);
        table.begin(1000, 0);
        assert!((0..1000).all(|index| !table.contains(index)));
        let mut whole = SearchTable::with_all(vec![5u8; 300]);
        whole.begin(300, 0);
        assert!((0..300).all(|index| !whole.contains(index)));
    }
}
