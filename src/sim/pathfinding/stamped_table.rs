//! Search scratch that forgets every entry at once.

/// Per-index values that one search writes and reads, reused across searches.
///
/// An entry belongs to the running search only while its stamp equals the
/// serial, so [`Self::begin`] starts a search in O(1) instead of clearing a
/// table as long as the map or the zone graph: nothing an earlier search wrote
/// counts. Pure scratch: never saved or hashed.
#[derive(Debug)]
pub(super) struct StampedTable<T> {
    stamps: Vec<u32>,
    values: Vec<T>,
    /// The running search's stamp. Never 0, the value every stamp starts at.
    serial: u32,
}

impl<T> Default for StampedTable<T> {
    fn default() -> Self {
        Self {
            stamps: Vec::new(),
            values: Vec::new(),
            serial: 0,
        }
    }
}

impl<T: Copy> StampedTable<T> {
    /// Start a search over indices `0..len`, forgetting every entry. Slots
    /// that grow the table hold `blank`, which no search reads.
    pub(super) fn begin(&mut self, len: usize, blank: T) {
        if self.stamps.len() < len {
            self.stamps.resize(len, 0);
            self.values.resize(len, blank);
        }
        self.serial = self.serial.wrapping_add(1);
        if self.serial == 0 {
            // After 2^32 searches the serial would repeat: clear every stamp.
            self.stamps.fill(0);
            self.serial = 1;
        }
    }

    pub(super) fn contains(&self, index: usize) -> bool {
        self.stamps[index] == self.serial
    }

    /// The running search's entry at `index`, if it wrote one.
    pub(super) fn get(&self, index: usize) -> Option<T> {
        self.contains(index).then(|| self.values[index])
    }

    pub(super) fn set(&mut self, index: usize, value: T) {
        self.stamps[index] = self.serial;
        self.values[index] = value;
    }

    /// A table whose running search wrote every one of `values`.
    #[cfg(any(test, feature = "test-support"))]
    pub(super) fn with_all(values: Vec<T>) -> Self {
        Self {
            stamps: vec![1; values.len()],
            values,
            serial: 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::StampedTable;

    #[test]
    fn a_new_search_forgets_the_last_one() {
        let mut table = StampedTable::default();
        table.begin(4, 0);
        table.set(1, 7u8);
        assert_eq!(table.get(1), Some(7));
        assert_eq!(table.get(2), None);
        table.begin(8, 0);
        assert_eq!(table.get(1), None);
        table.set(6, 3);
        assert_eq!(table.get(6), Some(3));
    }

    #[test]
    fn a_wrapped_serial_forgets_every_entry() {
        let mut table = StampedTable::default();
        table.begin(4, 0);
        // Written by search 1, whose serial the wrap reuses.
        table.set(1, 7u8);
        table.serial = u32::MAX;
        table.set(2, 9);
        table.begin(4, 0);
        assert_eq!(table.serial, 1);
        assert_eq!(table.get(1), None);
        assert_eq!(table.get(2), None);
    }
}
