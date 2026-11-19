//! Who can shoot at a cell: one threat, one reach test, one index.
//!
//! Three passes used to carry this: `avoidance` weighed a route against the
//! guns that cover it, `kite` chose a step out of those guns, and `dodge` —
//! which is about shells already in the air, not guns — had a third type of
//! the same name. Three definitions of "inside the guns" is three answers to
//! one question, and the one that drifts is the one a play test reports.
//!
//! The shell in the air stays where it belongs, in `dodge`, under its own
//! name (`Shell`): it has a landing point and a deadline, not a reach.

use std::collections::BTreeMap;

/// The side of one index bucket, in cells.
pub(super) const BUCKET_CELLS: u16 = 16;

/// An enemy whose weapon can reach a cell, with what that costs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Threat {
    /// The object, so two threats in the same cell stay apart.
    pub(super) entity_id: u64,
    pub(super) cell: (u16, u16),
    /// What its weapon reaches, in cells.
    pub(super) reach: i32,
    /// `Strength=`, the currency both sides are weighed in.
    pub(super) points: i32,
}

impl Threat {
    /// A gun with no weight behind it — the question a caller asks when it
    /// only needs "does anything cover this cell".
    pub(super) fn gun(entity_id: u64, cell: (u16, u16), reach: i32) -> Self {
        Self {
            entity_id,
            cell,
            reach,
            points: 0,
        }
    }

    /// Whether this threat's guns cover `cell`.
    pub(super) fn covers(&self, cell: (u16, u16)) -> bool {
        covers(cell, self.cell, self.reach)
    }
}

/// Whether a weapon reaching `reach` cells from `origin` covers `cell`.
///
/// Chebyshev, because the game's adjacency is eight-way: a diagonal step is a
/// step, and a box is what a weapon's range is drawn in.
pub(super) fn covers(cell: (u16, u16), origin: (u16, u16), reach: i32) -> bool {
    distance(cell, origin) <= reach
}

/// Chebyshev distance in cells, as the reach test measures it.
pub(super) fn distance(a: (u16, u16), b: (u16, u16)) -> i32 {
    i32::from(a.0.abs_diff(b.0)).max(i32::from(a.1.abs_diff(b.1)))
}

/// Enemies bucketed by cell: a unit costs the enemies around it, not the
/// enemies on the map.
#[derive(Debug, Default)]
pub(super) struct ThreatIndex {
    buckets: BTreeMap<(u16, u16), Vec<Threat>>,
}

/// The bucket a cell belongs to.
pub(super) fn bucket_of(cell: (u16, u16)) -> (u16, u16) {
    (cell.0 / BUCKET_CELLS, cell.1 / BUCKET_CELLS)
}

impl ThreatIndex {
    pub(super) fn new() -> Self {
        Self::default()
    }

    pub(super) fn is_empty(&self) -> bool {
        self.buckets.is_empty()
    }

    pub(super) fn insert(&mut self, threat: Threat) {
        self.buckets
            .entry(bucket_of(threat.cell))
            .or_default()
            .push(threat);
    }

    /// Every enemy within `radius` cells of `cell`, nearest first, ties by id.
    pub(super) fn near(&self, cell: (u16, u16), radius: i32) -> Vec<Threat> {
        let span = i32::from(BUCKET_CELLS).max(1);
        let reach = u16::try_from((radius + span - 1) / span).unwrap_or(1);
        let (bx, by) = bucket_of(cell);
        let mut found = Vec::new();
        for y in by.saturating_sub(reach)..=by.saturating_add(reach) {
            for x in bx.saturating_sub(reach)..=bx.saturating_add(reach) {
                let Some(bucket) = self.buckets.get(&(x, y)) else {
                    continue;
                };
                found.extend(
                    bucket
                        .iter()
                        .copied()
                        .filter(|threat| distance(cell, threat.cell) <= radius),
                );
            }
        }
        found.sort_by_key(|threat| (distance(cell, threat.cell), threat.entity_id));
        found
    }

    /// The enemies whose guns cover `cell`, nearest first.
    pub(super) fn covering(&self, cell: (u16, u16), radius: i32) -> Vec<Threat> {
        self.near(cell, radius)
            .into_iter()
            .filter(|threat| threat.covers(cell))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The reach test is the one the game uses: eight-way, so a diagonal step
    /// counts as one cell.
    #[test]
    fn reach_is_chebyshev() {
        assert!(covers((10, 10), (13, 13), 3));
        assert!(!covers((10, 10), (14, 13), 3));
    }

    /// Two guns in the same cell stay apart, and the index answers nearest
    /// first with the id as the tie-break.
    #[test]
    fn the_index_answers_nearest_first() {
        let mut index = ThreatIndex::new();
        index.insert(Threat::gun(9, (12, 10), 4));
        index.insert(Threat::gun(7, (11, 10), 4));
        let near = index.near((10, 10), 12);
        assert_eq!(near.iter().map(|t| t.entity_id).collect::<Vec<_>>(), [7, 9]);
        assert_eq!(index.covering((10, 10), 12).len(), 2);
    }
}
