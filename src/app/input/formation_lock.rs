//! Formation lock: a VERA feature, gamemd has none
//! ([docs](../../../docs/formation-line.md)).
//!
//! The selected units remember where each stands relative to the others; a
//! plain move click then sends each to its own place around the clicked
//! cell, where gamemd would send them all to that cell and spread them
//! (`0x0064CDA0`, `sim::movement::group_destination`).
//!
//! The lock only chooses cells on this player's side. The orders are
//! ordinary Moves built by `super::context_order`; nothing is synced or
//! saved, so a lock lasts until the player unlocks it or the match ends.

use std::collections::HashMap;

use super::formation_line::Cell;

/// A locked unit's formation and its place in it: cells from the formation's
/// centre when it was locked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Slot {
    formation: u32,
    offset: Cell,
}

/// This player's locked formations, by unit.
#[derive(Debug, Default)]
pub(crate) struct FormationLocks {
    next_formation: u32,
    slots: HashMap<u64, Slot>,
}

impl FormationLocks {
    /// A new match: ids of the old one mean nothing.
    pub(crate) fn clear(&mut self) {
        self.slots.clear();
    }

    /// Forget units that no longer exist.
    pub(crate) fn retain(&mut self, mut alive: impl FnMut(u64) -> bool) {
        self.slots.retain(|&id, _| alive(id));
    }

    /// Every one of `ids` is locked, and into the same formation.
    pub(crate) fn is_one_formation(&self, ids: &[u64]) -> bool {
        let mut formations = ids
            .iter()
            .map(|id| self.slots.get(id).map(|slot| slot.formation));
        match formations.next() {
            Some(Some(first)) => formations.all(|formation| formation == Some(first)),
            _ => false,
        }
    }

    /// Lock `units`, each given with the cell it stands on or is heading
    /// for, into one new formation. A unit leaves any formation it was in.
    pub(crate) fn lock(&mut self, units: &[(u64, Cell)]) {
        let Some(centre) = centre(units.iter().map(|&(_, cell)| cell)) else {
            return;
        };
        let formation = self.next_formation;
        self.next_formation = self.next_formation.wrapping_add(1);
        for &(id, cell) in units {
            self.slots.insert(
                id,
                Slot {
                    formation,
                    offset: (cell.0 - centre.0, cell.1 - centre.1),
                },
            );
        }
    }

    pub(crate) fn unlock(&mut self, ids: &[u64]) {
        for id in ids {
            self.slots.remove(id);
        }
    }

    /// Where each unit goes for a move click on `click`. `units` are the
    /// click's receivers with the cells they stand on.
    ///
    /// `None`, an ordinary click, unless there are two or more and every one
    /// is locked. Each formation keeps its shape. Several formations ordered
    /// together also keep where they stand relative to each other, so two
    /// blocks side by side arrive side by side.
    pub(crate) fn move_cells(
        &self,
        units: &[(u64, Cell)],
        click: Cell,
    ) -> Option<HashMap<u64, Cell>> {
        if units.len() < 2 {
            return None;
        }
        let mut members: HashMap<u32, Vec<(Slot, Cell)>> = HashMap::new();
        for &(id, cell) in units {
            let slot = *self.slots.get(&id)?;
            members
                .entry(slot.formation)
                .or_default()
                .push((slot, cell));
        }
        let middle = centre(units.iter().map(|&(_, cell)| cell))?;
        // Each formation's centre now, and the centre of its members' places;
        // the second is off zero when only part of a formation is ordered.
        let anchors: HashMap<u32, (Cell, Cell)> = members
            .iter()
            .filter_map(|(&formation, members)| {
                let stands = centre(members.iter().map(|&(_, cell)| cell))?;
                let places = centre(members.iter().map(|&(slot, _)| slot.offset))?;
                Some((formation, (stands, places)))
            })
            .collect();
        units
            .iter()
            .map(|&(id, _)| {
                let slot = self.slots.get(&id)?;
                let &(stands, places) = anchors.get(&slot.formation)?;
                Some((
                    id,
                    (
                        click.0 + (stands.0 - middle.0) + (slot.offset.0 - places.0),
                        click.1 + (stands.1 - middle.1) + (slot.offset.1 - places.1),
                    ),
                ))
            })
            .collect()
    }
}

/// The middle of `cells`, rounded to a cell; `None` for no cells.
fn centre(cells: impl Iterator<Item = Cell>) -> Option<Cell> {
    let (mut x, mut y, mut count) = (0i64, 0i64, 0i64);
    for cell in cells {
        x += i64::from(cell.0);
        y += i64::from(cell.1);
        count += 1;
    }
    if count == 0 {
        return None;
    }
    let mean = |sum: i64| (sum * 2 + count).div_euclid(count * 2);
    Some((i32::try_from(mean(x)).ok()?, i32::try_from(mean(y)).ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_locked_group_keeps_its_shape_around_the_click_until_unlocked() {
        let mut locks = FormationLocks::default();
        locks.lock(&[(1, (10, 10)), (2, (12, 10)), (3, (11, 13))]);
        assert!(locks.is_one_formation(&[1, 2, 3]));
        // Wherever they stand now, each goes to its place around the click.
        let standing = [(1, (40, 2)), (2, (3, 3)), (3, (7, 60))];
        let cells = locks.move_cells(&standing, (30, 30)).unwrap();
        assert_eq!(cells[&1], (29, 29));
        assert_eq!(cells[&2], (31, 29));
        assert_eq!(cells[&3], (30, 32));

        // One unit outside the formation makes it an ordinary click.
        let mixed = [(1, (40, 2)), (2, (3, 3)), (9, (5, 5))];
        assert_eq!(locks.move_cells(&mixed, (30, 30)), None);
        locks.unlock(&[1, 2, 3]);
        assert!(!locks.is_one_formation(&[1, 2, 3]));
        assert_eq!(locks.move_cells(&standing, (30, 30)), None);
    }

    #[test]
    fn two_locked_groups_ordered_together_arrive_as_they_stand() {
        let mut locks = FormationLocks::default();
        let first = [(1, (10, 10)), (2, (10, 12))];
        let second = [(3, (20, 10)), (4, (20, 12))];
        locks.lock(&first);
        locks.lock(&second);
        assert!(!locks.is_one_formation(&[1, 2, 3, 4]));
        let all = [first, second].concat();
        let cells = locks.move_cells(&all, (50, 50)).unwrap();
        assert_eq!(
            [cells[&1], cells[&2], cells[&3], cells[&4]],
            [(45, 49), (45, 51), (55, 49), (55, 51)]
        );
    }
}
