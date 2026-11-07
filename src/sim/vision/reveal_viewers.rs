//! VERA-only: each owner's reveal viewers, remembered through the passes that
//! ask them for every moving source.

use std::collections::BTreeMap;
use std::sync::Arc;

use super::VisionConfig;
use crate::map::houses::HouseAllianceMap;
use crate::sim::intern::{InternedId, StringInterner};

/// Each owner's [`super::direct_reveal_viewers`] and [`super::allied_viewers`],
/// remembered from [`Self::begin`] to [`Self::end`], which bracket the object
/// pass and the fog pass. An answer depends on the owner, the client maps, the
/// ally graph, `AllyReveal=` and the interned names. A change of client maps
/// forgets every answer; inside the bracket names only grow, so their count
/// keys the answers, with `AllyReveal=` beside each direct one; the ally
/// graph's writers (`install_house_alliances`, `set_ally_bit`) forget them. Outside the bracket every ask computes afresh,
/// so a graph written between passes needs no forgetting. Callers pass the
/// Simulation's own client maps, alliances and interner. Debug builds check each
/// remembered answer against a fresh one. Never saved or hashed.
#[derive(Debug, Default)]
pub(crate) struct RevealViewers {
    remembering: bool,
    /// The interned names the answers were computed with.
    names: usize,
    /// The client maps the answers were computed among.
    clients: Arc<[InternedId]>,
    direct: BTreeMap<(InternedId, bool), Arc<[InternedId]>>,
    allied: BTreeMap<InternedId, Arc<[InternedId]>>,
}

impl RevealViewers {
    /// Start remembering, from no answers.
    pub(crate) fn begin(&mut self) {
        debug_assert!(!self.remembering, "one pass remembers at a time");
        self.forget();
        self.remembering = true;
    }

    /// Stop remembering, and drop the answers.
    pub(crate) fn end(&mut self) {
        self.forget();
        self.remembering = false;
    }

    /// Drop the answers: the ally graph changed.
    pub(crate) fn forget(&mut self) {
        self.direct.clear();
        self.allied.clear();
    }

    /// [`super::direct_reveal_viewers`] of `owner` among `clients`.
    pub(crate) fn direct(
        &mut self,
        alliances: &HouseAllianceMap,
        owner: InternedId,
        config: &VisionConfig,
        interner: &StringInterner,
        clients: &[InternedId],
    ) -> Arc<[InternedId]> {
        let fresh =
            || super::direct_reveal_viewers(alliances, owner, config, interner, clients).into();
        if !self.current(interner, clients) {
            return fresh();
        }
        let answer = Arc::clone(
            self.direct
                .entry((owner, config.ally_reveal))
                .or_insert_with(fresh),
        );
        debug_assert_eq!(
            answer,
            fresh(),
            "{owner:?}'s remembered viewers are current"
        );
        answer
    }

    /// [`super::allied_viewers`] of `owner` among `clients`.
    pub(crate) fn allied(
        &mut self,
        alliances: &HouseAllianceMap,
        owner: InternedId,
        interner: &StringInterner,
        clients: &[InternedId],
    ) -> Arc<[InternedId]> {
        let fresh = || super::allied_viewers(alliances, owner, interner, clients).into();
        if !self.current(interner, clients) {
            return fresh();
        }
        let answer = Arc::clone(self.allied.entry(owner).or_insert_with(fresh));
        debug_assert_eq!(answer, fresh(), "{owner:?}'s remembered allies are current");
        answer
    }

    /// Whether answers are remembered now, after dropping any computed with
    /// fewer names than there are or among other client maps.
    fn current(&mut self, interner: &StringInterner, clients: &[InternedId]) -> bool {
        if !self.remembering {
            return false;
        }
        if interner.len() != self.names || *self.clients != *clients {
            self.forget();
            self.names = interner.len();
            self.clients = clients.into();
        }
        true
    }
}
