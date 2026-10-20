//! Client-owned, ordered Techno history. EntityStore already visits ascending
//! stable IDs; exploit that order rather than hashing every captured transform.
//! Never indexed by a simulation slot (compaction/restore may change those).
use super::{Endpoints, RenderPose};
use std::collections::HashMap;

#[derive(Default)]
pub(super) struct PoseHistory {
    // IDs drive the ordered capture; the small lookup index changes only when
    // membership/order does. Its iteration order never reaches presentation.
    // Neither ID magnitude nor lifetime grows an indexed array.
    ids: Vec<u64>,
    rows: Vec<Endpoints>,
    slots: HashMap<u64, usize>,
    spare_ids: Vec<u64>,
    spare_rows: Vec<Endpoints>,
}

impl PoseHistory {
    pub(super) fn clear(&mut self) {
        self.ids.clear();
        self.rows.clear();
        self.slots.clear();
        self.spare_ids.clear();
        self.spare_rows.clear();
    }

    pub(super) fn get(&self, id: &u64) -> Option<&Endpoints> {
        self.slots.get(id).map(|&slot| &self.rows[slot])
    }

    /// `input` must be the unique, ascending IDs of the eligible live objects.
    /// Common case: update existing rows in place. Only the suffix after an
    /// insertion/removal/eligibility change needs a sorted merge. Missing IDs
    /// retire immediately, without retaining ghosts or scanning a hash table.
    pub(super) fn capture(
        &mut self,
        input: impl Iterator<Item = (u64, RenderPose)>,
        frame: u32,
        reset: bool,
    ) {
        let mut count = 0;
        let mut input = input;
        while let Some((id, current)) = input.next() {
            debug_assert!(count == 0 || self.ids[count - 1] < id);
            match self.ids.get(count) {
                Some(&old_id) if old_id == id => {
                    self.rows[count] =
                        Endpoints::next(Some(self.rows[count]), current, frame, reset);
                }
                None => {
                    self.slots.insert(id, count);
                    self.ids.push(id);
                    self.rows.push(Endpoints::next(None, current, frame, reset));
                }
                Some(_) => {
                    self.merge_suffix(
                        count,
                        std::iter::once((id, current)).chain(input),
                        frame,
                        reset,
                    );
                    return;
                }
            }
            count += 1;
        }
        for id in &self.ids[count..] {
            self.slots.remove(id);
        }
        self.ids.truncate(count);
        self.rows.truncate(count);
    }

    fn merge_suffix(
        &mut self,
        start: usize,
        input: impl Iterator<Item = (u64, RenderPose)>,
        frame: u32,
        reset: bool,
    ) {
        self.spare_ids.clear();
        self.spare_rows.clear();
        // The prefix was already advanced by capture; copy it, do not advance
        // it a second time. Old rows in the remaining suffix are still intact.
        self.spare_ids.extend_from_slice(&self.ids[..start]);
        self.spare_rows.extend_from_slice(&self.rows[..start]);
        let mut old = start;
        for (id, current) in input {
            debug_assert!(self.spare_ids.last().is_none_or(|last| *last < id));
            while old < self.ids.len() && self.ids[old] < id {
                self.slots.remove(&self.ids[old]);
                old += 1;
            }
            let previous = if self.ids.get(old) == Some(&id) {
                let row = self.rows[old];
                if old != self.spare_ids.len() {
                    self.slots.insert(id, self.spare_ids.len());
                }
                old += 1;
                Some(row)
            } else {
                self.slots.insert(id, self.spare_ids.len());
                None
            };
            self.spare_ids.push(id);
            self.spare_rows
                .push(Endpoints::next(previous, current, frame, reset));
        }
        for id in &self.ids[old..] {
            self.slots.remove(id);
        }
        std::mem::swap(&mut self.ids, &mut self.spare_ids);
        std::mem::swap(&mut self.rows, &mut self.spare_rows);
        self.spare_ids.clear();
        self.spare_rows.clear();
    }

    #[cfg(test)]
    pub(super) fn insert_for_test(&mut self, id: u64, row: Endpoints) {
        match self.ids.binary_search(&id) {
            Ok(slot) => self.rows[slot] = row,
            Err(slot) => {
                self.ids.insert(slot, id);
                self.rows.insert(slot, row);
                for (slot, &id) in self.ids.iter().enumerate().skip(slot) {
                    self.slots.insert(id, slot);
                }
            }
        }
    }

    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.ids.len()
    }
    pub(super) fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }
    #[cfg(test)]
    pub(super) fn contains_key(&self, id: &u64) -> bool {
        self.get(id).is_some()
    }
}

#[cfg(test)]
#[path = "history_tests.rs"]
mod tests;
