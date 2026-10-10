//! Retained native House spatial threat, distinct from damage AngerStruct scores.
//!
//! House509400 clears/rebuilds only at diplomacy boundaries; Cell481870 updates
//! incrementally at Techno lifecycle/Foot coarse-cell boundaries. Native reads
//! 56BCD0 and585F40 share this one persisted grid. No per-search census.
//! Evidence: original gamemd.exe bodies481870,4FA2E0,509400,56BC50,56BCD0,
//! 6F6AC0/6F6CA0,7014A0,70F670/70F6A0/70F6E0 and4D85D0.
//! Executed signed indices and wrapped nine-slot arithmetic:
//! tools/spatial_oracle/astar_threat_inputs.json, spatial controls.

use crate::map::entities::EntityCategory;
use crate::map::houses::{HouseAllianceMap, HouseRoster, is_allied_with};
use crate::rules::ruleset::RuleSet;
use crate::sim::intern::InternedId;
use crate::sim::world::Simulation;

const SIDE: i32 = 130;
const LENGTH: usize = 130 * 130;
// Original retail dwords8243C8/8243EC: row-major surrounding coarse cells.
const OFFSETS: [i32; 9] = [-131, -130, -129, -1, 0, 1, 129, 130, 131];
const SHIFTS: [u32; 9] = [2, 1, 2, 1, 0, 1, 2, 1, 2];

/// The native ScenarioInit counter is nonzero during ReadScenarioINI's House
/// pass (ordinary campaign prior2). CanAlly501540 then bypasses live-world
/// defeated/count gates; MakeAlly4F9B70 writes only the sender's ally mask.
pub(crate) enum HouseAllianceAdmission<'a> {
    ScenarioInitialization { roster: &'a HouseRoster },
    Admitted(HouseAllianceMap),
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(transparent)]
pub(crate) struct HouseSpatialThreat(Box<[i32]>);

impl Default for HouseSpatialThreat {
    fn default() -> Self {
        Self(vec![0; LENGTH].into_boxed_slice())
    }
}

impl<'de> serde::Deserialize<'de> for HouseSpatialThreat {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let values = <Vec<i32> as serde::Deserialize>::deserialize(deserializer)?;
        if values.len() != LENGTH {
            return Err(serde::de::Error::custom(
                "native House threat grid must contain 16900 dwords",
            ));
        }
        Ok(Self(values.into_boxed_slice()))
    }
}

impl HouseSpatialThreat {
    pub(super) fn values(&self) -> &[i32] {
        &self.0
    }

    pub(crate) fn index(cell: (i16, i16)) -> i32 {
        // 56BC50 writer includes the 131-dword padding. 56BCD0's +59F0 base
        // is exactly +57E4 +131*4. Hierarchy581F90/584550/585F40 uses the
        // same signed packed-cell mapping; all consumers share this owner.
        // Rust signed division truncates toward zero.
        131 + i32::from(cell.0) / 4 + SIDE * (i32::from(cell.1) / 4)
    }

    pub(super) fn at(&self, cell: (i16, i16)) -> Result<i32, String> {
        let index = Self::index(cell);
        self.at_padded_index(index)
    }

    pub(super) fn at_padded_index(&self, index: i32) -> Result<i32, String> {
        usize::try_from(index)
            .ok()
            .and_then(|index| self.0.get(index))
            .copied()
            .ok_or_else(|| {
                format!("native House threat lookup outside retained grid at padded index {index}")
            })
    }

    pub(super) fn clear(&mut self) {
        self.0.fill(0);
    }

    pub(super) fn adjust(&mut self, cell: (i16, i16), delta: i32) -> Result<(), String> {
        let base = Self::index(cell);
        // Validate dependencies before the first write. Extreme scalar
        // coordinates may alias unrelated native House bytes; no invented zero.
        let first = base + OFFSETS[0];
        let last = base + OFFSETS[8];
        if first < 0 || last >= LENGTH as i32 {
            return Err(format!(
                "native House threat kernel outside retained grid at {cell:?}"
            ));
        }
        // 4FA2E7 tests sign BEFORE NEG. MIN remains negative after wrapped NEG;
        // each SAR and ADD/SUB is signed32, followed by its own zero clamp.
        let magnitude = delta.wrapping_abs();
        for (offset, shift) in OFFSETS.into_iter().zip(SHIFTS) {
            let value = &mut self.0[(base + offset) as usize];
            let part = magnitude >> shift;
            *value = if delta < 0 {
                value.wrapping_sub(part)
            } else {
                value.wrapping_add(part)
            }
            .max(0);
        }
        Ok(())
    }
}

impl Simulation {
    /// 56BCD0: resolve the Cell through the canonical fixed-stride/dummy owner
    /// before reading that Cell's retained signed coordinate and the House map.
    pub(crate) fn house_threat_at_cell(
        &self,
        owner: InternedId,
        coord: (i16, i16),
    ) -> Result<i32, String> {
        let terrain = self
            .resolved_terrain
            .as_ref()
            .ok_or("House threat lookup requires map cells")?;
        let coord = terrain.native_cell_coord(terrain.native_cell_identity(coord));
        self.houses
            .get(&owner)
            .ok_or("House threat lookup requires a live House")?
            .spatial_threat_at(coord)
    }

    fn spatial_threat_current_cell(
        &self,
        id: u64,
        terrain: Option<&crate::map::resolved_terrain::ResolvedTerrainGrid>,
    ) -> Option<(i16, i16)> {
        let actor = self.substrate.entities.get(id)?;
        let terrain = terrain.or(self.resolved_terrain.as_ref())?;
        let coord = (actor.position.rx as i16, actor.position.ry as i16);
        Some(terrain.native_cell_coord(terrain.native_cell_identity(coord)))
    }

    fn live_spatial_threat(&self, id: u64, rules: &RuleSet) -> Option<i32> {
        let actor = self.substrate.entities.get(id)?;
        let object = rules.object(self.interner.resolve(actor.type_ref()))?;
        Some(crate::sim::combat::live_threat_posed(
            actor,
            Some(object),
            &self.substrate.entities,
            rules,
            &self.interner,
        ))
    }

    /// Cell481870: each receiver excludes the source House and its directional
    /// allies. Unlike509400's rebuild, this applies to every concrete class.
    fn cell_update_spatial_threat(&mut self, source: InternedId, cell: (i16, i16), delta: i32) {
        let source_name = self.interner.resolve(source);
        for (&owner, house) in &mut self.houses {
            if owner == source
                || is_allied_with(
                    &self.house_alliances,
                    self.interner.resolve(owner),
                    source_name,
                )
            {
                continue;
            }
            house
                .adjust_spatial_threat(cell, delta)
                .expect("native Cell UpdateThreat requires an addressable House kernel");
        }
    }

    fn add_spatial_threat_at(&mut self, id: u64, cell: (i16, i16), rules: &RuleSet) {
        let Some(value) = self.live_spatial_threat(id, rules) else {
            return;
        };
        let actor = self.substrate.entities.get_mut(id).unwrap();
        let owner = actor.owner();
        actor.retain_spatial_threat(value);
        self.cell_update_spatial_threat(owner, cell, value);
    }

    fn remove_spatial_threat_at(&mut self, id: u64, cell: (i16, i16)) {
        let actor = self.substrate.entities.get(id).unwrap();
        let owner = actor.owner();
        let value = actor
            .cached_spatial_threat()
            .expect("required native Techno+508 read before an admitted threat publication");
        self.cell_update_spatial_threat(owner, cell, value.wrapping_neg());
        self.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .retain_spatial_threat(0);
    }

    /// TechnoUnlimbo6F6EB7..6F6EDE, after successful Reveal and the alive gate,
    /// before Foot Unlimbo discovery/neighbors. Dead successful Reveal retains
    /// the constructor-uninitialized cache. Map-less fixtures have no Cell.
    pub(crate) fn spatial_threat_after_unlimbo(
        &mut self,
        id: u64,
        rules: &RuleSet,
        terrain: Option<&crate::map::resolved_terrain::ResolvedTerrainGrid>,
    ) {
        if self
            .substrate
            .entities
            .get(id)
            .is_some_and(|actor| actor.lifecycle.object_alive)
            && let Some(cell) = self.spatial_threat_current_cell(id, terrain)
        {
            self.add_spatial_threat_at(id, cell, rules);
        }
    }

    /// TechnoLimbo6F6BDA..6F6C2F: guard on current positive live threat,
    /// remove the retained value at Foot+55C (else current Cell), clear before
    /// ObjectConceal. A failed Conceal must not roll these writes back.
    pub(crate) fn spatial_threat_before_limbo(
        &mut self,
        id: u64,
        rules: &RuleSet,
        terrain: Option<&crate::map::resolved_terrain::ResolvedTerrainGrid>,
    ) {
        let Some(actor) = self.substrate.entities.get(id) else {
            return;
        };
        if actor.lifecycle.in_limbo
            || self
                .live_spatial_threat(id, rules)
                .is_none_or(|value| value <= 0)
        {
            return;
        }
        let cell = if actor.category == EntityCategory::Structure {
            self.spatial_threat_current_cell(id, terrain)
        } else {
            terrain.or(self.resolved_terrain.as_ref()).map(|terrain| {
                terrain.native_cell_coord(
                    terrain.native_cell_identity(actor.navigation.neighbor_state.cell()),
                )
            })
        };
        if let Some(cell) = cell {
            self.remove_spatial_threat_at(id, cell);
        }
    }

    /// Techno701701..701719, after detach and before writing the new owner.
    /// Return the retained Cell so the post-owner half cannot resample it.
    pub(crate) fn spatial_threat_before_owner_change(&mut self, id: u64) -> Option<(i16, i16)> {
        let actor = self.substrate.entities.get(id)?;
        if actor.lifecycle.in_limbo {
            return None;
        }
        let cell = if actor.category == EntityCategory::Structure {
            self.spatial_threat_current_cell(id, None)
        } else {
            self.resolved_terrain.as_ref().map(|terrain| {
                terrain.native_cell_coord(
                    terrain.native_cell_identity(actor.navigation.neighbor_state.cell()),
                )
            })
        }?;
        self.remove_spatial_threat_at(id, cell);
        Some(cell)
    }

    /// Techno701769..701780 uses the saved Cell and newly committed owner.
    pub(crate) fn spatial_threat_after_owner_change(
        &mut self,
        id: u64,
        cell: Option<(i16, i16)>,
        rules: &RuleSet,
    ) {
        if let Some(cell) = cell {
            self.add_spatial_threat_at(id, cell, rules);
        }
    }

    /// Foot4D8657..4D868D, BEFORE the existing +55C neighbor-history writer.
    /// Same bucket keeps both cache and map untouched, even if live threat
    /// changed. Zero history skips this whole prefix, like neighbor migration.
    pub(crate) fn spatial_threat_at_per_cell(&mut self, id: u64, rules: &RuleSet) {
        let Some(actor) = self.substrate.entities.get(id) else {
            return;
        };
        if actor.category == EntityCategory::Structure {
            return;
        }
        let old = actor.navigation.neighbor_state.cell();
        if old == (0, 0) {
            return;
        }
        let current = (actor.position.rx as i16, actor.position.ry as i16);
        if HouseSpatialThreat::index(old) == HouseSpatialThreat::index(current) {
            return;
        }
        let Some(terrain) = self.resolved_terrain.as_ref() else {
            return;
        };
        let old = terrain.native_cell_coord(terrain.native_cell_identity(old));
        self.remove_spatial_threat_at(id, old);
        if let Some(current) = self.spatial_threat_current_cell(id, None) {
            self.add_spatial_threat_at(id, current, rules);
        }
    }

    ///70F6E0 refresh: garrison append52298D and ejection4581D6/458732 own
    /// their exact invocation boundaries. Health changes alone do not call it.
    pub(crate) fn refresh_spatial_threat(
        &mut self,
        id: u64,
        rules: &RuleSet,
        terrain: Option<&crate::map::resolved_terrain::ResolvedTerrainGrid>,
    ) {
        if let Some(cell) = self.spatial_threat_current_cell(id, terrain) {
            self.remove_spatial_threat_at(id, cell);
            self.add_spatial_threat_at(id, cell, rules);
        }
    }

    ///509400 is an explicit rebuild at diplomacy, never a navigation cache.
    pub(crate) fn rebuild_house_spatial_threat(&mut self, owner: InternedId, rules: &RuleSet) {
        let Some(house) = self.houses.get(&owner) else {
            return;
        };
        let controlled = house.is_controlled_by_human(self.session.game_mode_nonzero);
        // Stable IDs are monotonic construction IDs, preserving Techno array
        // order independently of active Logic membership (including limbo).
        let mut contributions = Vec::new();
        for (id, actor) in self.substrate.entities.iter_sorted().rev() {
            if actor.lifecycle.in_limbo || !actor.lifecycle.object_alive {
                continue;
            }
            let Some(value) = self
                .live_spatial_threat(id, rules)
                .filter(|value| *value > 0)
            else {
                continue;
            };
            let cell = if actor.category == EntityCategory::Structure {
                // Native non-Foot arm intentionally has no house/ally filter.
                self.spatial_threat_current_cell(id, None)
            } else {
                let cell = actor.navigation.neighbor_state.cell();
                if cell == (0, 0)
                    || actor.owner() == owner
                    || (controlled
                        && is_allied_with(
                            &self.house_alliances,
                            self.interner.resolve(owner),
                            self.interner.resolve(actor.owner()),
                        ))
                {
                    continue;
                }
                Some(cell) //56BC50 directly reads Foot+55C; no GetCell here.
            };
            if let Some(cell) = cell {
                contributions.push((cell, value));
            }
        }
        let house = self.houses.get_mut(&owner).unwrap();
        house.clear_spatial_threat();
        for (cell, value) in contributions {
            house
                .adjust_spatial_threat(cell, value)
                .expect("native diplomacy rebuild requires an addressable House kernel");
        }
    }

    /// Existing launch diplomacy graph owner: commit its admitted graph, then
    /// rebuild only recipients whose ally bits changed, in House array order.
    /// This retains the threat dependency of4F9BB9/4FA062/4FA0D5. The fresh
    /// ScenarioInit arm precedes every Techno, so live target/anger/paranoid
    /// sight effects cannot run. Live CanAlly/EVA remain outside that bound.
    pub(crate) fn install_house_alliances(
        &mut self,
        admission: HouseAllianceAdmission<'_>,
        rules: &RuleSet,
    ) {
        let alliances = match admission {
            HouseAllianceAdmission::Admitted(alliances) => alliances,
            HouseAllianceAdmission::ScenarioInitialization { roster } => {
                assert!(
                    self.entities().is_empty() && self.production.terrain_objects.is_empty(),
                    "ScenarioInit diplomacy precedes map objects"
                );
                assert!(
                    !self.session.game_mode_nonzero,
                    "this ScenarioInit admission is the campaign map-read family"
                );
                assert_eq!(
                    roster.houses.len(),
                    self.session.house_order.len(),
                    "House ReadScenarioINI follows complete House construction"
                );
                roster.alliance_map()
            }
        };
        let changed: Vec<_> = self
            .session
            .house_order
            .iter()
            .copied()
            .filter(|owner| {
                let key = self.interner.resolve(*owner).trim().to_ascii_uppercase();
                self.house_alliances
                    .get(&key)
                    .into_iter()
                    .flatten()
                    .ne(alliances.get(&key).into_iter().flatten())
            })
            .collect();
        self.house_alliances = alliances;
        for owner in changed {
            self.rebuild_house_spatial_threat(owner, rules);
        }
    }
}

#[cfg(test)]
#[path = "house_threat_tests.rs"]
mod tests;
