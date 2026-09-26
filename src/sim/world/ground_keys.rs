//! Kept Ground sort keys: what GetYSort reads of the Ground display members
//! stays valid between comparisons.
//!
//! A Ground submit compares the new object with the members in order until
//! one sorts after it (`DisplayLayers::submit`), and the per-frame adjacent
//! pass compares every neighbour. With many objects that is millions of key
//! reads a frame, almost all unchanged since the last read. This cache keeps
//! what a read says may be kept, and the Simulation forgets it wherever the
//! stores' touch logs say its inputs may have changed (`display_registry`),
//! so a comparison sees exactly what a live read returns. Debug builds compare
//! every kept read with a live one.
//!
//! Derived: never saved, compared or hashed. A restored display keeps nothing.

use std::collections::HashMap;

use crate::sim::touch_log::{Touched, live_read_check_enabled};

/// What a Ground member's last read left valid until the member is forgotten.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KeptSortKey {
    /// Nothing: read the member again at its next comparison.
    Unknown,
    /// The member's key.
    Key(i32),
    /// An attached member's own term. Its key adds `owner`'s term, which is
    /// kept per owner and forgotten with the owner.
    Attached { own: i32, owner: u64 },
}

/// GetYSort for Ground comparisons. Reads are pure. Production scans read
/// through `display_registry`'s view, which forgets what changed first.
pub(crate) trait GroundSortKeys {
    /// `id`'s key read live, and what of it may be kept.
    fn read(&self, id: u64) -> (i32, KeptSortKey);
    /// The term an [`KeptSortKey::Attached`] member adds for `owner`, read live.
    fn owner_term(&self, owner: u64) -> i32;
}

/// A test's plain key function keeps nothing. Members an earlier scan kept
/// are still compared by their kept reads.
#[cfg(test)]
impl<F: Fn(u64) -> i32> GroundSortKeys for F {
    fn read(&self, id: u64) -> (i32, KeptSortKey) {
        (self(id), KeptSortKey::Unknown)
    }

    fn owner_term(&self, _owner: u64) -> i32 {
        unreachable!("a plain key function keeps no attached member")
    }
}

/// The kept reads. Members reach theirs through `slots`, which moves in
/// lockstep with the Ground vector, so forgetting one member is O(1) however
/// the members insert, leave and swap.
#[derive(Debug, Default, Clone)]
pub(crate) struct GroundKeys {
    /// Aligned with the Ground members: each member's index into `kept`.
    slots: Vec<u32>,
    kept: Vec<KeptSortKey>,
    /// Slots of departed members, for reuse.
    free: Vec<u32>,
    /// Ground member -> its slot.
    slot_of: HashMap<u64, u32>,
    /// Owner -> its term, for attached members.
    owner_terms: HashMap<u64, i32>,
    /// What every kept read was read under (see [`Self::forget_changed`]).
    context: Option<ReadContext>,
}

/// The inputs every read shares: the rules and the type handle table a
/// Building's key resolves its type through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ReadContext {
    /// By address, never dereferenced.
    pub(crate) rules: Option<usize>,
    pub(crate) type_handles: u64,
}

impl GroundKeys {
    /// Nothing kept for `members`.
    pub(crate) fn unknown(members: &[u64]) -> Self {
        Self {
            slots: (0..members.len() as u32).collect(),
            kept: vec![KeptSortKey::Unknown; members.len()],
            free: Vec::new(),
            slot_of: members
                .iter()
                .enumerate()
                .map(|(slot, &id)| (id, slot as u32))
                .collect(),
            owner_terms: HashMap::new(),
            context: None,
        }
    }

    /// The key of member `id`, which sits at `index`.
    pub(crate) fn key(&mut self, index: usize, id: u64, keys: &impl GroundSortKeys) -> i32 {
        let slot = self.slots[index] as usize;
        let key = match self.kept[slot] {
            KeptSortKey::Key(key) => key,
            KeptSortKey::Attached { own, owner } => {
                let term = match self.owner_terms.get(&owner) {
                    Some(&term) => term,
                    None => {
                        let term = keys.owner_term(owner);
                        self.owner_terms.insert(owner, term);
                        term
                    }
                };
                own.wrapping_add(term)
            }
            KeptSortKey::Unknown => {
                let (key, kept) = keys.read(id);
                self.keep(slot, key, kept);
                return key;
            }
        };
        debug_assert!(
            !live_read_check_enabled() || key == keys.read(id).0,
            "the kept Ground key of {id} diverged from a live read"
        );
        key
    }

    /// Store what a live read of `key` may keep; an attached read's owner
    /// term is its key less its own term.
    fn keep(&mut self, slot: usize, key: i32, kept: KeptSortKey) {
        self.kept[slot] = kept;
        if let KeptSortKey::Attached { own, owner } = kept {
            self.owner_terms.insert(owner, key.wrapping_sub(own));
        }
    }

    /// Member `id` joins at `index`; its live read returned `key` and `kept`.
    pub(crate) fn insert(&mut self, index: usize, id: u64, key: i32, kept: KeptSortKey) {
        let slot = self.free.pop().unwrap_or_else(|| {
            self.kept.push(KeptSortKey::Unknown);
            (self.kept.len() - 1) as u32
        });
        self.keep(slot as usize, key, kept);
        self.slots.insert(index, slot);
        self.slot_of.insert(id, slot);
    }

    /// Member `id`, which sits at `index`, leaves.
    pub(crate) fn remove(&mut self, index: usize, id: u64) {
        let slot = self.slots.remove(index);
        debug_assert_eq!(
            self.slot_of.get(&id),
            Some(&slot),
            "Ground member {id} left through another member's slot"
        );
        self.kept[slot as usize] = KeptSortKey::Unknown;
        self.free.push(slot);
        self.slot_of.remove(&id);
    }

    /// The members at `a` and `b` trade places.
    pub(crate) fn swap(&mut self, a: usize, b: usize) {
        self.slots.swap(a, b);
    }

    /// Forget every read whose inputs may have changed since the last call:
    /// all of them when the reads' context differs or a store reports
    /// everything, else the reads of the ids the stores handed out mutably
    /// (as members and as owners).
    pub(crate) fn forget_changed(
        &mut self,
        context: ReadContext,
        touched: impl IntoIterator<Item = Touched>,
    ) {
        if self.context != Some(context) {
            self.context = Some(context);
            self.forget_all();
        }
        for touched in touched {
            match touched {
                Touched::All => self.forget_all(),
                Touched::Ids(ids) => {
                    for id in ids {
                        if let Some(&slot) = self.slot_of.get(&id) {
                            self.kept[slot as usize] = KeptSortKey::Unknown;
                        }
                        self.owner_terms.remove(&id);
                    }
                }
            }
        }
    }

    fn forget_all(&mut self) {
        self.kept.fill(KeptSortKey::Unknown);
        self.owner_terms.clear();
    }

    /// How many members the slots cover.
    pub(crate) fn len(&self) -> usize {
        self.slots.len()
    }

    /// Whether the slots cover exactly `members`, each through its own slot.
    #[cfg(test)]
    pub(crate) fn aligned_with(&self, members: &[u64]) -> bool {
        self.slots.len() == members.len()
            && self.slot_of.len() == members.len()
            && members
                .iter()
                .zip(&self.slots)
                .all(|(id, &slot)| self.slot_of.get(id) == Some(&slot))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::world::display_layers::{DisplayLayer, DisplayLayers};

    /// Objects with an own term; some are attached to an owner whose term the
    /// key adds.
    #[derive(Default)]
    struct Scene {
        own: HashMap<u64, i32>,
        owner_of: HashMap<u64, u64>,
        owner_terms: HashMap<u64, i32>,
    }

    impl Scene {
        fn live(&self, id: u64) -> i32 {
            let term = self
                .owner_of
                .get(&id)
                .map_or(0, |owner| self.owner_terms.get(owner).copied().unwrap_or(0));
            self.own[&id].wrapping_add(term)
        }
    }

    impl GroundSortKeys for Scene {
        fn read(&self, id: u64) -> (i32, KeptSortKey) {
            let key = self.live(id);
            let kept = match self.owner_of.get(&id) {
                Some(&owner) => KeptSortKey::Attached {
                    own: self.own[&id],
                    owner,
                },
                None => KeptSortKey::Key(key),
            };
            (key, kept)
        }

        fn owner_term(&self, owner: u64) -> i32 {
            self.owner_terms.get(&owner).copied().unwrap_or(0)
        }
    }

    const CONTEXT: ReadContext = ReadContext {
        rules: None,
        type_handles: 0,
    };

    /// A deterministic sequence of submits, departures, adjacent passes, own
    /// moves and owner moves gives the same layers whether the reads are kept
    /// (and forgotten for what moved) or made live every time; every kept read
    /// is also checked against a live one inside `GroundKeys::key`.
    #[test]
    fn kept_reads_order_ground_exactly_as_live_reads() {
        let mut scene = Scene::default();
        let mut kept = DisplayLayers::default();
        let mut live = DisplayLayers::default();
        let mut state: u64 = 0x5EED;
        let mut next = |bound: u64| {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (state >> 33) % bound
        };
        for id in 1..=40u64 {
            scene.own.insert(id, (next(64) as i32 - 32) * 16);
            if id % 3 == 0 {
                scene.owner_of.insert(id, 100 + next(4));
            }
        }
        for owner in 100..104 {
            scene.owner_terms.insert(owner, next(512) as i32);
        }
        kept.forget_changed_ground_keys(CONTEXT, []);
        for step in 0..3000 {
            let id = 1 + next(40);
            match next(6) {
                0 | 1 => {
                    let layer = if next(5) == 0 {
                        DisplayLayer::AIR
                    } else {
                        DisplayLayer::GROUND
                    };
                    kept.submit(id, Some(layer), &scene);
                    live.submit(id, Some(layer), &|id| scene.live(id));
                }
                2 => {
                    kept.remove(id);
                    live.remove(id);
                }
                3 => {
                    kept.sort_ground_pass(&scene);
                    live.sort_ground_pass(&|id| scene.live(id));
                }
                4 => {
                    *scene.own.get_mut(&id).unwrap() += (next(9) as i32 - 4) * 64;
                    kept.forget_changed_ground_keys(CONTEXT, [Touched::Ids(vec![id])]);
                }
                _ => {
                    let owner = 100 + next(4);
                    *scene.owner_terms.get_mut(&owner).unwrap() += (next(9) as i32 - 4) * 64;
                    kept.forget_changed_ground_keys(CONTEXT, [Touched::Ids(vec![owner])]);
                }
            }
            for layer in [DisplayLayer::GROUND, DisplayLayer::AIR] {
                assert_eq!(kept.members(layer), live.members(layer), "step {step}");
            }
            assert!(kept.ground_keys_aligned(), "step {step}");
        }
    }

    /// A different read context, or a store reporting everything, forgets
    /// every kept read.
    #[test]
    fn a_new_context_or_everything_forgets_all_reads() {
        let mut scene = Scene::default();
        for id in 1..=6u64 {
            scene.own.insert(id, id as i32 * 100);
        }
        scene.owner_of.insert(6, 50);
        scene.owner_terms.insert(50, 0);
        for forget in [
            (
                ReadContext {
                    rules: Some(8),
                    type_handles: 0,
                },
                Touched::Ids(Vec::new()),
            ),
            (CONTEXT, Touched::All),
        ] {
            let mut display = DisplayLayers::default();
            display.forget_changed_ground_keys(CONTEXT, []);
            for id in 1..=6 {
                display.submit(id, Some(DisplayLayer::GROUND), &scene);
            }
            assert_eq!(display.members(DisplayLayer::GROUND), [1, 2, 3, 4, 5, 6]);
            // Everything moves without a per-id note: only the blanket
            // forget below makes the next pass read the new keys.
            let mut moved = Scene::default();
            for id in 1..=6u64 {
                moved.own.insert(id, 700 - id as i32 * 100);
            }
            moved.owner_of.insert(6, 50);
            moved.owner_terms.insert(50, 0);
            let (context, touched) = forget.clone();
            display.forget_changed_ground_keys(context, [touched]);
            display.sort_ground_pass(&moved);
            assert_eq!(display.members(DisplayLayer::GROUND), [2, 3, 4, 5, 6, 1]);
        }
    }
}
