//! What the house can see, and what it remembers having seen.
//!
//! The census reads every enemy on the map. A player sees what the shroud
//! shows and remembers the last place an object was when it went out of sight;
//! the difference is the whole of "the AI cheats", and switching the census off
//! without a memory behind it is a brain that forgets a base the moment a
//! rifleman walks past it.
//!
//! This is that memory, and it is deliberately a separate owner from the
//! census: the census answers "what exists", this answers "what is known".
//! `AUTOPLAY_HONEST=1` swaps the brain's enemy list for this one; the default
//! keeps the omniscient list while the policy is moved onto it.
//!
//! Staleness is a frame budget, not a fade: an object seen at `t` is a fact
//! about `t` only, and after [`MEMORY_FRAMES`] it is forgotten rather than
//! tracked with an invented position. A remembered object keeps the cell it
//! was seen at — the honest answer is "it was here", never "it is here".

use std::collections::BTreeMap;

use crate::sim::control::observation::EnemyEntity;

/// How long a sighting is remembered, in frames: thirty seconds. Long enough
/// that a base the army walked past is still a target when the army comes
/// back, short enough that a remembered cell is not an argument for a fight
/// the house has not seen the ground for.
pub const MEMORY_FRAMES: u64 = 450;

/// One object as the house last knew it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Remembered {
    pub entity: EnemyEntity,
    /// The frame it was last seen on.
    pub seen_at: u64,
}

/// The house's picture: what it saw this frame, and what it remembers.
#[derive(Debug, Default)]
pub struct Perception {
    memory: BTreeMap<u64, Remembered>,
}

impl Perception {
    pub fn new() -> Self {
        Self::default()
    }

    /// Take this frame's sightings, forget what has aged out and invalidate
    /// last-seen positions which have been *visibly* disproved.
    ///
    /// `visible` is the projection's shroud test: the caller decides what the
    /// house can see, this only keeps the answer. A sighting that has gone out
    /// of sight keeps its last cell and its last frame; the term is what stops
    /// the memory being a second omniscient list.
    pub fn observe(
        &mut self,
        now: u64,
        enemies: impl Iterator<Item = EnemyEntity>,
        visible: impl Fn((u16, u16)) -> bool,
    ) -> usize {
        let mut seen = 0usize;
        for enemy in enemies {
            if !visible(enemy.cell) {
                continue;
            }
            seen += 1;
            self.memory.insert(
                enemy.entity_id,
                Remembered {
                    entity: enemy,
                    seen_at: now,
                },
            );
        }
        self.memory.retain(|_, remembered| {
            // Seen this tick: the entity still exists at this position. Not
            // seen this tick, but its last known tile is visible again: it
            // has left, changed owner or died. Do not aim the next assault at
            // a confirmed empty plot for another 450 frames. An unseen tile
            // still retains the last known position until the normal expiry.
            now.saturating_sub(remembered.seen_at) < MEMORY_FRAMES
                && (remembered.seen_at == now || !visible(remembered.entity.cell))
        });
        seen
    }

    /// What the house may act on now: everything it can see, plus what it
    /// remembers within the term, nearest first by id so the order is stable.
    pub fn enemies(&self) -> Vec<EnemyEntity> {
        self.memory
            .values()
            .map(|remembered| remembered.entity)
            .collect()
    }

    pub fn is_empty(&self) -> bool {
        self.memory.is_empty()
    }

    pub fn len(&self) -> usize {
        self.memory.len()
    }

    /// Whether the house is acting on a memory rather than a sighting: the
    /// reading that says how much of the picture is remembered.
    pub fn remembered_only(&self, now: u64) -> usize {
        self.memory
            .values()
            .filter(|remembered| remembered.seen_at < now)
            .count()
    }
}

#[cfg(all(test, tests_sim_c_g))]
mod tests {
    use super::*;
    use crate::sim::intern::InternedId;

    fn enemy(id: u64, cell: (u16, u16)) -> EnemyEntity {
        EnemyEntity {
            entity_id: id,
            type_id: InternedId::from_index(1),
            cell,
        }
    }

    /// A sighting is remembered after it leaves the shroud, and for the term
    /// only: a memory that never expires is an omniscient list with extra
    /// steps.
    #[test]
    fn a_sighting_is_remembered_for_its_term() {
        let mut perception = Perception::new();
        let mut visible = true;
        assert_eq!(
            perception.observe(0, [enemy(7, (10, 10))].into_iter(), |_| visible),
            1
        );
        assert_eq!(perception.enemies().len(), 1);
        // Out of sight: still a fact about frame 0, so still remembered.
        visible = false;
        assert_eq!(
            perception.observe(100, [enemy(7, (12, 12))].into_iter(), |_| visible),
            0
        );
        assert_eq!(
            perception.enemies().first().map(|e| e.cell),
            Some((10, 10)),
            "the remembered cell is where it was seen, not where it might be"
        );
        assert_eq!(perception.remembered_only(100), 1);
        // Past the term it is forgotten.
        perception.observe(MEMORY_FRAMES + 1, std::iter::empty(), |_| false);
        assert!(perception.enemies().is_empty());
    }

    /// A vanished target may remain in fog, but *not* after the army scouts
    /// its previous position and finds the tile empty again.
    #[test]
    fn a_visible_empty_previous_position_invalidates_attack_memory() {
        let mut perception = Perception::new();
        perception.observe(10, [enemy(7, (10, 10))].into_iter(), |_| true);
        perception.observe(11, std::iter::empty(), |_| false);
        assert_eq!(perception.enemies().len(), 1);
        perception.observe(12, std::iter::empty(), |cell| cell == (10, 10));
        assert!(perception.enemies().is_empty());
    }

    #[test]
    fn a_fresh_sighting_survives_a_visible_tile_and_replaces_stale_position() {
        let mut perception = Perception::new();
        perception.observe(10, [enemy(7, (10, 10))].into_iter(), |_| true);
        perception.observe(11, [enemy(7, (14, 10))].into_iter(), |_| true);
        assert_eq!(perception.enemies().first().map(|e| e.cell), Some((14, 10)));
        assert_eq!(perception.remembered_only(11), 0);
    }

    /// Seeing it again refreshes the term and the cell.
    #[test]
    fn a_second_sighting_refreshes_the_memory() {
        let mut perception = Perception::new();
        perception.observe(0, [enemy(7, (10, 10))].into_iter(), |_| true);
        perception.observe(400, [enemy(7, (30, 30))].into_iter(), |_| true);
        perception.observe(600, std::iter::empty(), |_| false);
        assert_eq!(
            perception.enemies().first().map(|e| e.cell),
            Some((30, 30)),
            "the later sighting is the one remembered"
        );
    }
}
