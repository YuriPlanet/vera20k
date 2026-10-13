//! Map577D90/577AB0 bulk transitions bracketed by4ADEE0/4ADCD0.
//! Native callbacks visit Techno registration order, ordinary sight then Gap on
//! each object. House240 idempotence is distinct from House577A SpySatActive;
//! Map577F30 (RevealEntireMap) shares the House240 latch.
use super::{FogState, InternedId, OwnerVisibility, SightAdmission, gap_footprint};

impl FogState {
    /// House+240, the existing Map reveal/gap authority. The separately
    /// stored House+241 map-clear byte does not decide reveal admission.
    pub(crate) fn whole_map_revealed_for_owner(&self, owner: InternedId) -> bool {
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

    #[cfg(test)]
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
                (viewer == owner && source.owner == owner).then_some(id)
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

    #[cfg(test)]
    pub(crate) fn transition_whole_map_with_sources(
        &mut self,
        owner: InternedId,
        cells: Vec<(u16, u16)>,
        reset: bool,
        old_spy_sat_active: bool,
        source_ids: &std::collections::BTreeSet<u64>,
    ) {
        self.transition_whole_map_with_gap_reentry(
            owner,
            cells,
            reset,
            old_spy_sat_active,
            source_ids,
            None,
            &std::collections::BTreeMap::new(),
        );
    }

    /// `alternates` are the sources the re-admission callback's
    /// `RevealToAll=` branch reveals on this map, with their footprints.
    pub(crate) fn transition_whole_map_with_gap_reentry(
        &mut self,
        owner: InternedId,
        cells: Vec<(u16, u16)>,
        reset: bool,
        old_spy_sat_active: bool,
        source_ids: &std::collections::BTreeSet<u64>,
        gap_reentry: Option<&[super::GapGeneratorSource]>,
        alternates: &std::collections::BTreeMap<u64, SightAdmission>,
    ) {
        if self.width == 0
            || self.height == 0
            || (!reset && self.whole_map_revealed_owners.contains(&owner))
        {
            return;
        }
        let sources: std::collections::BTreeMap<_, _> = self
            .sight_admissions
            .iter()
            .filter(|(key, _)| key.1 == owner && source_ids.contains(&key.0))
            .map(|(key, value)| (key.0, value.clone()))
            .collect();
        let gaps = self.gap_sources.get(&owner).cloned().unwrap_or_default();
        let mut gaps_by_id: std::collections::BTreeMap<u64, Vec<_>> =
            std::collections::BTreeMap::new();
        for gap in gaps {
            gaps_by_id.entry(gap.stable_id).or_default().push(gap);
        }
        let mut reentry_by_id = gaps_by_id.clone();
        if let Some(reentry) = gap_reentry {
            reentry_by_id.clear();
            for &gap in reentry {
                reentry_by_id.entry(gap.stable_id).or_default().push(gap);
            }
        }
        // `0x004ADEE0` has no `RevealToAll=` branch: an alternate admission
        // keeps its source count through the bulk store.
        let ids: std::collections::BTreeSet<_> = sources
            .keys()
            .copied()
            .chain(gaps_by_id.keys().copied())
            .chain(reentry_by_id.keys().copied())
            .chain(alternates.keys().copied())
            .collect();
        let width = usize::from(self.width);
        let height = usize::from(self.height);
        self.by_owner
            .entry(owner)
            .or_insert_with(|| OwnerVisibility::new(self.width, self.height));
        //4ADEE0, per-object release then remove-gap. Activation sees OLD577A
        //false, deactivation OLD577Atrue; the House owner changes it afterwards.
        for &id in &ids {
            if sources.contains_key(&id) {
                self.release_sight_for_viewer((id, owner));
            }
            for &gap in gaps_by_id.get(&id).into_iter().flatten() {
                let vis = self.by_owner.get_mut(&owner).unwrap();
                for index in gap_footprint(gap, width, height) {
                    vis.shroud_knowledge[index].remove_gap(old_spy_sat_active);
                }
            }
        }
        self.gap_sources.remove(&owner);
        if !reset {
            self.whole_map_revealed_owners.insert(owner);
        }
        {
            let vis = self.by_owner.get_mut(&owner).unwrap();
            for (x, y) in cells {
                let Some(index) = vis.index(x, y) else {
                    continue;
                };
                let state = &mut vis.shroud_knowledge[index];
                state.counter = if reset { 1 } else { 0 };
                state.gap_counter = 0;
                state.open = !reset;
                // Both original bulk stores preserve Cell140 pending20.
                vis.publish_knowledge(index);
            }
        }
        //4ADCD0 repeats registration order, ordinary sight then gap. Stable IDs
        //are monotonic Techno construction identities in the current owner.
        for id in ids {
            if let Some(source) = sources.get(&id) {
                self.reconcile_sight_admission(id, owner, source.clone(), false);
            }
            if let Some(footprint) = alternates.get(&id) {
                self.admit_alternate_sight(id, owner, footprint.clone());
            }
            for &gap in reentry_by_id.get(&id).into_iter().flatten() {
                let vis = self.by_owner.get_mut(&owner).unwrap();
                for index in gap_footprint(gap, width, height) {
                    vis.shroud_knowledge[index].add_gap();
                    vis.publish_knowledge(index);
                }
                self.gap_sources.entry(owner).or_default().insert(gap);
                //Complete6FB170 clears local House240 after its cell loop.
                self.whole_map_revealed_owners.remove(&owner);
            }
        }
        //577AB0 clears240 only after callback re-admission.
        if reset {
            self.whole_map_revealed_owners.remove(&owner);
        }
    }
}
