//! Map577D90/577AB0 bulk transitions bracketed by4ADEE0/4ADCD0.
//! Native callbacks visit Techno registration order, ordinary sight then Gap on
//! each object. House240 idempotence is distinct from House577A SpySatActive;
//! Map577F30 (RevealEntireMap) shares the House240 latch.
use super::{FogState, InternedId, OwnerVisibility};
#[cfg(any(test, feature = "test-support"))]
use super::{SightAdmission, gap_footprint};

/// The three native bulk writers have different counter semantics.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum MapTransition {
    Reveal,
    ResetShroud,
    Reshroud,
}

impl FogState {
    /// House+240, the existing Map reveal/gap authority. The separately
    /// stored House+241 map-clear byte does not decide reveal admission.
    pub fn whole_map_revealed_for_owner(&self, owner: InternedId) -> bool {
        self.whole_map_revealed_owners.contains(&owner)
    }

    /// `MapClass::RevealEntireMap`'s one-time House+240 test and set
    /// (`0x00577F9A..0x00577FAD`): false when the map is already revealed.
    pub(crate) fn admit_whole_map_reveal(&mut self, owner: InternedId) -> bool {
        self.whole_map_revealed_owners.insert(owner)
    }

    /// Successful native6FB170 gap re-admission invalidates House+240,
    /// including a friendly generator whose footprint leaves no hostile gap.
    pub(crate) fn invalidate_whole_map_reveal_for_owner(&mut self, owner: InternedId) {
        self.whole_map_revealed_owners.remove(&owner);
    }

    pub(crate) fn hash_whole_map_reveal_state(&self, hasher: &mut impl std::hash::Hasher) {
        std::hash::Hash::hash(&self.whole_map_revealed_owners, hasher);
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn transition_whole_map_for_owner(
        &mut self,
        owner: InternedId,
        cells: Vec<(u16, u16)>,
        reset: bool,
        old_spy_sat_active: bool,
    ) {
        // Synthetic/map-bootstrap callers have no live object registry. Only
        // own-viewer admitted sources belong to their represented callback set.
        let source_ids = self
            .sight_admissions
            .iter()
            .filter_map(|(&(id, viewer), source)| {
                (viewer == owner && source.owner == owner && source.latched).then_some(id)
            })
            .collect();
        self.transition_whole_map_with_sources(
            owner,
            cells,
            reset,
            old_spy_sat_active,
            &source_ids,
        );
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn transition_whole_map_with_sources(
        &mut self,
        owner: InternedId,
        cells: Vec<(u16, u16)>,
        reset: bool,
        old_spy_sat_active: bool,
        source_ids: &std::collections::BTreeSet<u64>,
    ) {
        // Each source is re-admitted with the footprint it holds.
        let sources = source_ids
            .iter()
            .map(|&id| (id, self.sight_admissions.get(&(id, owner)).cloned()))
            .collect();
        let mut world = crate::sim::world::Simulation::default();
        world.fog = std::mem::take(self);
        world.transition_whole_map_with_gap_reentry(
            owner,
            cells,
            if reset {
                MapTransition::ResetShroud
            } else {
                MapTransition::Reveal
            },
            old_spy_sat_active,
            &sources,
            None,
            &std::collections::BTreeMap::new(),
            &super::VisionConfig::default(),
            crate::sim::world::UninitContext::default(),
        );
        *self = world.fog;
    }

    /// Map577D90's entry240 guard, before the shared live callbacks. The
    /// reset writers577AB0/577C90 always enter. Source: original gamemd.
    pub(crate) fn begin_map_transition(
        &mut self,
        owner: InternedId,
        transition: MapTransition,
    ) -> bool {
        if self.width == 0
            || self.height == 0
            || (transition == MapTransition::Reveal
                && self.whole_map_revealed_owners.contains(&owner))
        {
            return false;
        }
        self.by_owner
            .get_or_insert_with(owner, || OwnerVisibility::new(self.width, self.height))
            .tracked
            .complete = false;
        true
    }

    /// Explicit footprint-only fixture inputs; no live source admission or
    /// native House/Techno decision is implemented here.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn prepare_map_transition(
        &mut self,
        owner: InternedId,
        transition: MapTransition,
        sources: &std::collections::BTreeMap<u64, Option<SightAdmission>>,
        gap_reentry: Option<&[super::GapGeneratorSource]>,
        alternates: &std::collections::BTreeMap<u64, SightAdmission>,
    ) -> Option<MapTransitionPlan> {
        if !self.begin_map_transition(owner, transition) {
            return None;
        }
        let held: std::collections::BTreeSet<_> = self
            .sight_admissions
            .iter()
            .filter(|(key, source)| {
                key.1 == owner && source.latched && sources.contains_key(&key.0)
            })
            .map(|(key, _)| key.0)
            .collect();
        let mut gaps = std::collections::BTreeMap::<u64, Vec<super::GapGeneratorSource>>::new();
        for gap in self.gap_sources.get(&owner).into_iter().flatten() {
            gaps.entry(gap.stable_id).or_default().push(*gap);
        }
        let mut reentry = gaps.clone();
        if let Some(current) = gap_reentry {
            reentry.clear();
            for &gap in current {
                reentry.entry(gap.stable_id).or_default().push(gap);
            }
        }
        let ids = held
            .iter()
            .copied()
            .chain(gaps.keys().copied())
            .chain(reentry.keys().copied())
            .chain(alternates.keys().copied())
            .collect();
        Some(MapTransitionPlan {
            held,
            #[cfg(any(test, feature = "test-support"))]
            gaps,
            #[cfg(any(test, feature = "test-support"))]
            reentry,
            ids,
        })
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn remove_map_transition_gap(
        &mut self,
        owner: InternedId,
        gap: super::GapGeneratorSource,
        old_spy_sat_active: bool,
    ) {
        let vis = self
            .by_owner
            .get_mut(&owner)
            .expect("admitted map transition");
        for index in gap_footprint(gap, usize::from(self.width), usize::from(self.height)) {
            vis.remove_gap_cell(index, old_spy_sat_active);
        }
    }

    pub(crate) fn store_map_transition(
        &mut self,
        owner: InternedId,
        cells: Vec<(u16, u16)>,
        transition: MapTransition,
    ) {
        self.gap_sources.remove(&owner);
        if transition == MapTransition::Reveal {
            self.whole_map_revealed_owners.insert(owner);
        }
        let vis = self
            .by_owner
            .get_mut(&owner)
            .expect("admitted map transition");
        for cell in cells {
            if transition == MapTransition::Reshroud {
                vis.store_reshrouded_map_cell(cell);
            } else {
                vis.store_bulk_map_cell(cell, transition != MapTransition::Reveal);
            }
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn reenter_map_transition_gap(
        &mut self,
        owner: InternedId,
        gap: super::GapGeneratorSource,
    ) {
        let vis = self
            .by_owner
            .get_mut(&owner)
            .expect("admitted map transition");
        for index in gap_footprint(gap, usize::from(self.width), usize::from(self.height)) {
            vis.add_gap_cell(index);
        }
        self.gap_sources.entry(owner).or_default().insert(gap);
        self.whole_map_revealed_owners.remove(&owner);
    }
}

#[cfg(any(test, feature = "test-support"))]
pub(crate) struct MapTransitionPlan {
    pub held: std::collections::BTreeSet<u64>,
    #[cfg(any(test, feature = "test-support"))]
    pub gaps: std::collections::BTreeMap<u64, Vec<super::GapGeneratorSource>>,
    #[cfg(any(test, feature = "test-support"))]
    pub reentry: std::collections::BTreeMap<u64, Vec<super::GapGeneratorSource>>,
    pub ids: std::collections::BTreeSet<u64>,
}
