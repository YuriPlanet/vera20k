//! Result rules for authored network operations using the shared House owners.

use std::collections::BTreeMap;

use crate::rules::ruleset::RuleSet;

use super::house_state::HouseOutcomeKind;
use super::intern::InternedId;
use super::world::Simulation;

impl Simulation {
    /// The admitted primary Houses in map order, even after a source leaves.
    pub(crate) fn authored_participants(&self) -> impl Iterator<Item = InternedId> + '_ {
        self.session
            .authored_primary_house_indices()
            .iter()
            .filter_map(|&index| self.houses.order().get(usize::from(index)).copied())
    }

    /// Match-wide participant identity from the frozen primary House indices.
    pub(super) fn is_authored_participant(&self, house: InternedId) -> bool {
        self.authored_participants().any(|player| player == house)
    }

    /// VMCE extension of TAction6DEA37/6DEA67's PlayerPtr Country test.
    /// One reciprocal alliance with a direct Country match is sufficient.
    pub(super) fn authored_country_outcome_recipients(
        &self,
        players: &[InternedId],
        country_index: i32,
        rules: &RuleSet,
    ) -> Vec<InternedId> {
        let matching: Vec<_> = players
            .iter()
            .copied()
            .filter(|&owner| self.outcome_country_index(owner, rules) == Some(country_index))
            .collect();
        players
            .iter()
            .copied()
            .filter(|&owner| {
                matching.iter().any(|&other| {
                    owner == other
                        || (self.house_alliances.is_allied(&self.interner, owner, other)
                            && self.house_alliances.is_allied(&self.interner, other, owner))
                })
            })
            .collect()
    }

    /// The first completed participant verdict retires this observation.
    /// The observer's own House remains outside the result and its timer.
    pub(super) fn follow_authored_participant_outcome(
        &mut self,
        owner: InternedId,
        kind: HouseOutcomeKind,
    ) {
        if self.session.outcome_request().is_none()
            && self
                .session
                .current_house()
                .is_some_and(|local| self.session.is_observer_house(local))
            && self.is_authored_participant(owner)
        {
            self.session.admit_outcome(kind);
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct AuthoredOutcomes {
    pub(super) primary_house_indices: Vec<u16>,
    // The House flags and timer own each result. This shared latch stops later
    // producers from replacing their first collective verdict.
    locked: bool,
    // View-only receipt from the House completion owner. The native victory
    // path clears its pending flag after the wait; other seats need to be
    // able to display that verdict even when their own result takes longer.
    completed: BTreeMap<InternedId, HouseOutcomeKind>,
}

impl AuthoredOutcomes {
    pub(super) fn new(mut primary_house_indices: Vec<u16>) -> Self {
        primary_house_indices.sort_unstable();
        Self {
            primary_house_indices,
            locked: false,
            completed: BTreeMap::new(),
        }
    }

    pub(super) fn lock(&mut self) -> bool {
        !std::mem::replace(&mut self.locked, true)
    }

    pub(super) fn is_locked(&self) -> bool {
        self.locked
    }

    pub(super) fn complete(&mut self, owner: InternedId, kind: HouseOutcomeKind) {
        self.completed.insert(owner, kind);
    }

    pub(super) fn completed(&self, owner: InternedId) -> Option<HouseOutcomeKind> {
        self.completed.get(&owner).copied()
    }
}
