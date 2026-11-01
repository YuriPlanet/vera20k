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
use crate::rules::ruleset::RuleSet;
use crate::sim::house_diplomacy::HouseAlliances;
use crate::sim::houses::Houses;
use crate::sim::intern::InternedId;
use crate::sim::intern::StringInterner;
use crate::sim::world::ObjectSubstrate;
use crate::sim::world::Simulation;

/// Techno `+0x508`, the threat this object last published to its enemies'
/// spatial maps. The constructor leaves the dword unwritten
/// (`0x006F310C..0x006F311E`); `None` keeps that domain until the first
/// publication. Publishing stores the value ([`Self::publish`]: Unlimbo
/// `0x006F6ED2`, `AddCellThreat` `0x0070F689`, ChangeOwner `0x00701774`,
/// `UpdateThreatInCell` `0x0070F71B`) and withdrawing stores 0
/// ([`Self::withdraw`]: Limbo `0x006F6C2F`, `RemoveCellThreat` `0x0070F6C2`,
/// ChangeOwner `0x00701719`, `UpdateThreatInCell` `0x0070F706`). It is
/// independent of the current type and garrison threat and survives saves.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub(crate) struct SpatialThreat(Option<i32>);

impl SpatialThreat {
    /// The last published threat, or `None` before the first publication.
    pub(crate) const fn get(self) -> Option<i32> {
        self.0
    }

    /// Store the threat just added to the maps.
    pub(crate) fn publish(&mut self, value: i32) {
        self.0 = Some(value);
    }

    /// Store 0 after the threat is removed from the maps.
    pub(crate) fn withdraw(&mut self) {
        self.0 = Some(0);
    }
}

// One coarse cell covers four map cells; retain the native one-cell border.
// Large maps use the same kernel with a larger map-selected plane.
pub(crate) const THREAT_GRID_SIDE: i32 = 130;
const SIDE: i32 = THREAT_GRID_SIDE;
const LENGTH: usize = (SIDE * SIDE) as usize;
const SHIFTS: [u32; 9] = [2, 1, 2, 1, 0, 1, 2, 1, 2];

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
        if crate::util::save_codec::layout::describing() {
            return Ok(Self::default());
        }
        if values.len() != LENGTH && values.len() != 258 * 258 {
            return Err(serde::de::Error::custom(format!(
                "House threat grid must contain {LENGTH} or 66564 dwords"
            )));
        }
        Ok(Self(values.into_boxed_slice()))
    }
}

impl HouseSpatialThreat {
    fn grid(&self) -> crate::map::cell_index::CellGrid {
        if self.0.len() == LENGTH {
            crate::map::cell_index::CellGrid::Retail
        } else {
            crate::map::cell_index::CellGrid::Extended
        }
    }

    pub(crate) fn configure(&mut self, grid: crate::map::cell_index::CellGrid) {
        let old_side = usize::from(self.grid().side()) / 4 + 2;
        let side = usize::from(grid.side()) / 4 + 2;
        if old_side == side {
            return;
        }
        let mut resized = vec![0; side * side];
        for y in 0..old_side.min(side) {
            for x in 0..old_side.min(side) {
                resized[y * side + x] = self.0[y * old_side + x];
            }
        }
        self.0 = resized.into_boxed_slice();
    }

    pub(crate) fn index_for_grid(grid: crate::map::cell_index::CellGrid, cell: (i16, i16)) -> i32 {
        let side = i32::from(grid.side()) / 4 + 2;
        side + 1 + i32::from(cell.0) / 4 + side * (i32::from(cell.1) / 4)
    }

    pub(super) fn values(&self) -> &[i32] {
        &self.0
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn index(cell: (i16, i16)) -> i32 {
        // 56BC50 writer includes the 131-dword padding. 56BCD0's +59F0 base
        // is exactly +57E4 +131*4. Hierarchy581F90/584550/585F40 uses the
        // same signed packed-cell mapping; all consumers share this owner.
        // Rust signed division truncates toward zero.
        Self::index_for_grid(crate::map::cell_index::CellGrid::Retail, cell)
    }

    pub(super) fn at(&self, cell: (i16, i16)) -> Result<i32, String> {
        let index = Self::index_for_grid(self.grid(), cell);
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
        let base = Self::index_for_grid(self.grid(), cell);
        // Validate dependencies before the first write. Extreme scalar
        // coordinates may alias unrelated native House bytes; no invented zero.
        let first = base - (i32::from(self.grid().side()) / 4 + 3);
        let last = base + (i32::from(self.grid().side()) / 4 + 3);
        if first < 0 || last >= self.0.len() as i32 {
            return Err(format!(
                "native House threat kernel outside retained grid at {cell:?}"
            ));
        }
        // 4FA2E7 tests sign BEFORE NEG. MIN remains negative after wrapped NEG;
        // each SAR and ADD/SUB is signed32, followed by its own zero clamp.
        let magnitude = delta.wrapping_abs();
        let side = i32::from(self.grid().side()) / 4 + 2;
        let offsets = [
            -side - 1,
            -side,
            -side + 1,
            -1,
            0,
            1,
            side - 1,
            side,
            side + 1,
        ];
        for (offset, shift) in offsets.into_iter().zip(SHIFTS) {
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
        let coord = (actor.position.rx() as i16, actor.position.ry() as i16);
        Some(terrain.native_cell_coord(terrain.native_cell_identity(coord)))
    }

    fn live_spatial_threat(&self, id: u64, rules: &RuleSet) -> Option<i32> {
        let actor = self.substrate.entities.get(id)?;
        let object = self.interner.object_type_in_category(
            actor.type_ref(),
            actor.category().into(),
            rules,
        )?;
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
    /// Every other House takes the write, clamp included. The walk goes in
    /// id order, not the House array's (0x00A8022C); each House writes only
    /// its own grid, so the results are the same.
    fn cell_update_spatial_threat(
        interner: &StringInterner,
        house_alliances: &HouseAlliances,
        houses: &mut Houses,
        source: InternedId,
        cell: (i16, i16),
        delta: i32,
    ) {
        // Each receiver asks its own IsAlliedWith(source): the index overload
        // 0x004F9A10, called at 0x004818B7.
        let friendly = house_alliances.allied_to(interner, source);
        for (&owner, house) in &mut *houses {
            if friendly(owner) {
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
        actor.cached_spatial_threat.publish(value);
        Self::cell_update_spatial_threat(
            &self.interner,
            &self.house_alliances,
            &mut self.houses,
            owner,
            cell,
            value,
        );
    }

    fn remove_spatial_threat_at(
        interner: &StringInterner,
        house_alliances: &HouseAlliances,
        substrate: &mut ObjectSubstrate,
        houses: &mut Houses,
        id: u64,
        cell: (i16, i16),
    ) {
        let actor = substrate.entities.get(id).unwrap();
        let owner = actor.owner();
        let value = actor
            .cached_spatial_threat()
            .expect("required native Techno+508 read before an admitted threat publication");
        Self::cell_update_spatial_threat(
            interner,
            house_alliances,
            houses,
            owner,
            cell,
            value.wrapping_neg(),
        );
        substrate
            .entities
            .get_mut(id)
            .unwrap()
            .cached_spatial_threat
            .withdraw();
    }

    /// TechnoUnlimbo6F6EB7..6F6EDE, after successful Reveal and the alive gate,
    /// before Foot Unlimbo discovery/neighbors. The caller carries the single
    /// pre-sight-callback6F6D04 admission; this writer does not recheck alive.
    /// Initially dead successful Reveal retains the constructor-uninitialized
    /// cache. Missing objects and map-less fixtures have no current Cell.
    pub(crate) fn spatial_threat_after_unlimbo(
        &mut self,
        id: u64,
        rules: &RuleSet,
        terrain: Option<&crate::map::resolved_terrain::ResolvedTerrainGrid>,
    ) {
        if let Some(cell) = self.spatial_threat_current_cell(id, terrain) {
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
        if actor.lifecycle.in_limbo()
            || self
                .live_spatial_threat(id, rules)
                .is_none_or(|value| value <= 0)
        {
            return;
        }
        let cell = if actor.category() == EntityCategory::Structure {
            self.spatial_threat_current_cell(id, terrain)
        } else {
            terrain.or(self.resolved_terrain.as_ref()).map(|terrain| {
                terrain.native_cell_coord(
                    terrain.native_cell_identity(actor.navigation.neighbor_state().cell()),
                )
            })
        };
        if let Some(cell) = cell {
            Self::remove_spatial_threat_at(
                &self.interner,
                &self.house_alliances,
                &mut self.substrate,
                &mut self.houses,
                id,
                cell,
            );
        }
    }

    /// Techno701701..701719, after detach and before writing the new owner.
    /// Return the retained Cell so the post-owner half cannot resample it.
    pub(crate) fn spatial_threat_before_owner_change(&mut self, id: u64) -> Option<(i16, i16)> {
        let actor = self.substrate.entities.get(id)?;
        if actor.lifecycle.in_limbo() {
            return None;
        }
        let cell = if actor.category() == EntityCategory::Structure {
            self.spatial_threat_current_cell(id, None)
        } else {
            self.resolved_terrain.as_ref().map(|terrain| {
                terrain.native_cell_coord(
                    terrain.native_cell_identity(actor.navigation.neighbor_state().cell()),
                )
            })
        }?;
        Self::remove_spatial_threat_at(
            &self.interner,
            &self.house_alliances,
            &mut self.substrate,
            &mut self.houses,
            id,
            cell,
        );
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
        if actor.category() == EntityCategory::Structure {
            return;
        }
        let old = actor.navigation.neighbor_state().cell();
        if old == (0, 0) {
            return;
        }
        let current = (actor.position.rx() as i16, actor.position.ry() as i16);
        if HouseSpatialThreat::index_for_grid(self.cell_grid(), old)
            == HouseSpatialThreat::index_for_grid(self.cell_grid(), current)
        {
            return;
        }
        let Some(terrain) = self.resolved_terrain.as_ref() else {
            return;
        };
        let old = terrain.native_cell_coord(terrain.native_cell_identity(old));
        Self::remove_spatial_threat_at(
            &self.interner,
            &self.house_alliances,
            &mut self.substrate,
            &mut self.houses,
            id,
            old,
        );
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
            Self::remove_spatial_threat_at(
                &self.interner,
                &self.house_alliances,
                &mut self.substrate,
                &mut self.houses,
                id,
                cell,
            );
            self.add_spatial_threat_at(id, cell, rules);
        }
    }

    ///509400 is an explicit rebuild at diplomacy, never a navigation cache.
    pub(crate) fn rebuild_house_spatial_threat(&mut self, owner: InternedId, rules: &RuleSet) {
        let Some(house) = self.houses.get(&owner) else {
            return;
        };
        let controlled = house.is_controlled_by_human(self.session.game_mode_nonzero());
        let contributions = self.spatial_threat_contributions(owner, controlled, rules);
        let house = self.houses.get_mut(&owner).unwrap();
        house.clear_spatial_threat();
        for (cell, value) in contributions {
            house
                .adjust_spatial_threat(cell, value)
                .expect("native diplomacy rebuild requires an addressable House kernel");
        }
    }
}

impl Simulation {
    /// What each object adds to `owner`'s rebuilt spatial threat, in the
    /// rebuild's order: descending stable id, which as monotonic construction
    /// ids keeps Techno array order independently of active Logic membership
    /// (including limbo). VERA-only: the objects are read side by side, each
    /// detached from the shared Dummy; when one would stamp it (a building
    /// whose cell is off the map), the walk runs again one by one on the
    /// shared Dummy, as native leaves it.
    fn spatial_threat_contributions(
        &self,
        owner: InternedId,
        controlled: bool,
        rules: &RuleSet,
    ) -> Vec<((i16, i16), i32)> {
        let contribution = |id: u64, actor: &crate::sim::game_entity::GameEntity| {
            if actor.lifecycle.in_limbo() || !actor.lifecycle.object_alive() {
                return None;
            }
            let value = self
                .live_spatial_threat(id, rules)
                .filter(|value| *value > 0)?;
            let cell = if actor.category() == EntityCategory::Structure {
                // Native non-Foot arm intentionally has no house/ally filter.
                self.spatial_threat_current_cell(id, None)?
            } else {
                let cell = actor.navigation.neighbor_state().cell();
                if cell == (0, 0)
                    || actor.owner() == owner
                    || (controlled
                        && self
                            .house_alliances
                            .is_allied(&self.interner, owner, actor.owner()))
                {
                    return None;
                }
                cell //56BC50 directly reads Foot+55C; no GetCell here.
            };
            Some((cell, value))
        };
        let one_by_one = || {
            self.substrate
                .entities
                .iter_sorted()
                .rev()
                .filter_map(|(id, actor)| contribution(id, actor))
                .collect::<Vec<_>>()
        };
        let Some(terrain) = self.resolved_terrain.as_ref() else {
            return one_by_one();
        };
        let dummy = terrain.shared_cell_dummy();
        let read = self.substrate.entities.par_filter_map(|id, actor| {
            let (found, stamped) = dummy.detached_stamps(|| contribution(id, actor));
            (found.is_some() || stamped).then_some((found, stamped))
        });
        if read.iter().any(|&(_, stamped)| stamped) {
            return one_by_one();
        }
        let contributions: Vec<_> = read
            .into_iter()
            .rev()
            .filter_map(|(found, _)| found)
            .collect();
        debug_assert!(
            !crate::sim::touch_log::live_read_check_enabled() || contributions == one_by_one(),
            "the side-by-side threat census differs from the walk one by one"
        );
        contributions
    }
}

#[cfg(test)]
#[path = "house_threat_tests.rs"]
mod tests;
