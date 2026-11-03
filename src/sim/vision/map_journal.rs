//! Which viewers' maps a stretch of fog writes changed.
//!
//! A map is one viewer's share of the fog: its plane, the source caches
//! keyed by that viewer, its House+240 and hostile gap receipts, and each
//! Building's gap deposit (`TechnoClass+0x269`/`+0x26C`) for that viewer.
//! Between [`FogState::journal`] and [`FogState::end_journal`] each writer
//! keeps what it is about to change, the first time, so the end compares
//! values rather than counting writes: a release and its re-admission that
//! restore a map leave it unchanged. A write to fog state no map owns (a
//! fogged object, a reset) or a cell notification's receivers make the
//! outcome unknown.

use std::collections::{BTreeMap, BTreeSet};

use super::{FogState, GapGeneratorSource, InternedId, SightAdmission, SightSource};

/// What one map held before its first journaled write.
#[derive(Debug, Clone, Default)]
pub(super) struct MapJournal {
    /// Each map's House+240 and gap receipts.
    maps: BTreeMap<InternedId, (bool, Option<BTreeSet<GapGeneratorSource>>)>,
    /// Each source's caches for one map: `sight_sources`,
    /// `sight_admissions` and `alternate_sight_admissions`.
    sight: BTreeMap<(u64, InternedId), SightCaches>,
    /// Each Building gap deposit, as its writer reported it.
    deposits: BTreeMap<(u64, InternedId), Option<(bool, i32)>>,
    /// A write outside every map's state.
    shared: bool,
}

type SightCaches = (
    Option<SightSource>,
    Option<SightAdmission>,
    Option<SightAdmission>,
);

/// What a journaled stretch of fog writes did ([`FogState::end_journal`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct JournalOutcome {
    /// The maps written, changed or not.
    pub(crate) written: BTreeSet<InternedId>,
    /// The maps whose state differs from before the first write.
    pub(crate) changed: BTreeSet<InternedId>,
    /// Whether a write reached state no map owns, or a cell notification's
    /// receivers ran: what changed is then not known.
    pub(crate) shared: bool,
}

impl FogState {
    /// Start journaling which maps the fog writes change, until
    /// [`Self::end_journal`].
    pub(crate) fn journal(&mut self) {
        assert!(self.journal.is_none(), "fog journals do not nest");
        self.journal = Some(Box::default());
        self.by_owner.journal();
    }

    /// Whether the fog writes are journaled.
    pub(crate) fn journaling(&self) -> bool {
        self.journal.is_some()
    }

    /// Stop journaling. `deposit` reads `(id, viewer)`'s Building gap
    /// deposit as it is now.
    pub(crate) fn end_journal(
        &mut self,
        deposit: impl Fn(u64, InternedId) -> Option<(bool, i32)>,
    ) -> JournalOutcome {
        let journal = self.journal.take().expect("journaling");
        let mut outcome = JournalOutcome {
            shared: journal.shared,
            ..JournalOutcome::default()
        };
        for (viewer, changed) in self.by_owner.end_journal() {
            outcome.written.insert(viewer);
            if changed {
                outcome.changed.insert(viewer);
            }
        }
        for (viewer, before) in journal.maps {
            outcome.written.insert(viewer);
            if before != self.map_state(viewer) {
                outcome.changed.insert(viewer);
            }
        }
        for ((id, viewer), before) in journal.sight {
            outcome.written.insert(viewer);
            if before != self.sight_caches(id, viewer) {
                outcome.changed.insert(viewer);
            }
        }
        for ((id, viewer), before) in journal.deposits {
            outcome.written.insert(viewer);
            if before != deposit(id, viewer) {
                outcome.changed.insert(viewer);
            }
        }
        outcome
    }

    /// Before a write to `viewer`'s House+240 or gap receipts.
    pub(super) fn note_map(&mut self, viewer: InternedId) {
        if self
            .journal
            .as_ref()
            .is_some_and(|journal| !journal.maps.contains_key(&viewer))
        {
            let before = self.map_state(viewer);
            if let Some(journal) = &mut self.journal {
                journal.maps.insert(viewer, before);
            }
        }
    }

    /// Before a write to source `id`'s caches for `viewer`.
    pub(super) fn note_sight(&mut self, id: u64, viewer: InternedId) {
        if self
            .journal
            .as_ref()
            .is_some_and(|journal| !journal.sight.contains_key(&(id, viewer)))
        {
            let before = self.sight_caches(id, viewer);
            if let Some(journal) = &mut self.journal {
                journal.sight.insert((id, viewer), before);
            }
        }
    }

    /// Before the Building gap deposit writer changes `(id, viewer)`'s
    /// deposit, which holds `before`.
    pub(crate) fn note_gap_deposit(
        &mut self,
        id: u64,
        viewer: InternedId,
        before: Option<(bool, i32)>,
    ) {
        if let Some(journal) = &mut self.journal {
            journal.deposits.entry((id, viewer)).or_insert(before);
        }
    }

    /// Before a write outside every map's state, or a cell notification's
    /// receivers.
    pub(crate) fn note_shared_write(&mut self) {
        if let Some(journal) = &mut self.journal {
            journal.shared = true;
        }
    }

    fn map_state(&self, viewer: InternedId) -> (bool, Option<BTreeSet<GapGeneratorSource>>) {
        (
            self.whole_map_revealed_owners.contains(&viewer),
            self.gap_sources.get(&viewer).cloned(),
        )
    }

    fn sight_caches(&self, id: u64, viewer: InternedId) -> SightCaches {
        let key = (id, viewer);
        (
            self.sight_sources.get(&key).copied(),
            self.sight_admissions.get(&key).cloned(),
            self.alternate_sight_admissions.get(&key).cloned(),
        )
    }
}
