//! `Simulation` cache and navigation rebuilds: post-load cache restore, logic
//! membership, path and zone grids, recalculated-cell publication and wall
//! damage.

use std::collections::BTreeSet;
use std::sync::Arc;

use crate::map::entities::EntityCategory;
use crate::map::overlay::OverlayEntry;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::ruleset::RuleSet;
use crate::sim::ares_firestorm::FirestormZoneView;
use crate::sim::cell_kernel::WallGateConnections;
use crate::sim::overlay_grid::{
    OverlayGrid, damage_wall_overlay_with_runtime_host, recalc_overlay_passability_with_building,
    refresh_wall_neighborhood_with_host,
};
use crate::sim::pathfinding::PathGrid;
use crate::sim::pathfinding::terrain_speed;
use crate::sim::pathfinding::zone_incremental::ZoneRepairKind;

use super::ObjectKind;
use super::Simulation;
use super::SimulationWallRuntimeHost;
use super::navigation;
use super::repair_zone_after_recalc;

impl Simulation {
    /// Restore externally-derived cache fields after validated snapshot
    /// identity fixup and substrate re-registration.
    ///
    /// The caller must provide the same map/rules data that was used to initialize
    /// the original simulation. Cache fields were `#[serde(skip)]`'d and are at
    /// their Default values after deserialization. The caller must first run
    /// `restore_after_snapshot_load`, which resolves stable-ID references and
    /// rebuilds registry, LogicVector-membership, and CellClass-list caches.
    ///
    /// Overlay, bridge, and navigation authority are restored separately by
    /// `restore_map_authority_after_snapshot_load` once rules and the overlay
    /// registry are bound. Rule animation lists are re-interned from names:
    /// an outgoing simulation's numeric IDs do not belong to this saved world.
    pub(crate) fn rebuild_caches_after_load(
        &mut self,
        mut resolved_terrain: ResolvedTerrainGrid,
        terrain_speed_config: terrain_speed::TerrainSpeedConfig,
        rules: &RuleSet,
    ) {
        resolved_terrain.bind_shared_cell_dummy(self.shared_cell_dummy.clone());
        // Restore externally-derived data only. Substrate caches are rebuilt
        // transactionally by `restore_after_snapshot_load` before this call.
        // The supplied map grid predates runtime Terrain lifecycle changes, so
        // serialized Terrain objects must overwrite that derived projection.
        // Clear the map's derived projection, including cells outside saved
        // anchors. CellLoad483B9F restores its own heads independently of
        // current Type geometry or whether an instance ever had a footprint.
        let projected_cells: Vec<_> = resolved_terrain
            .cells()
            .iter()
            .filter(|cell| cell.terrain_object_occupation.is_some())
            .map(|cell| (cell.rx, cell.ry))
            .collect();
        for cell in projected_cells {
            resolved_terrain.set_terrain_object_occupation(cell, None);
        }
        // RecalcZone483C80 selects the first Terrain in saved cell order.
        // Replaying storage order would select the wrong surviving mask.
        for &cell in self.terrain_objects.cell_index().keys() {
            self.terrain_objects
                .project_cell_occupation(&mut resolved_terrain, cell);
        }
        // VERA terrain deformation follows the saved rules, as on a new map.
        resolved_terrain.set_terrain_deformation(rules.combat_damage.terrain_deformation);
        self.resolved_terrain = Some(resolved_terrain);
        self.terrain_speed_config = terrain_speed_config;
        self.resolve_rule_animation_lists(rules);
    }

    /// Rebuild LogicClass membership flags from the restored active order.
    ///
    /// `+0x98` is not serialized (native does not round-trip it); vector presence
    /// is authoritative: RegisterObject sets it after its insert (`0x0055BAC6`)
    /// and UnregisterObject clears it after its removal (`0x0055BB27`), so the
    /// saved order determines it. Idempotent — safe to call after any load.
    /// It takes no load arguments, so save/load membership is unit-testable.
    pub fn rebuild_logic_membership(&mut self) {
        for kind in ObjectKind::ALL {
            match kind {
                ObjectKind::Entity => {
                    for entity in self.substrate.entities.values_mut() {
                        entity.seldom.in_logic_vector.mark(false);
                    }
                }
                ObjectKind::Anim => {
                    for anim in self.substrate.anims.values_mut() {
                        anim.mark_logic_membership(false);
                    }
                }
                ObjectKind::VoxelAnim => {
                    for debris in self.substrate.voxel_anims.values_mut() {
                        debris.mark_logic_membership(false);
                    }
                }
                ObjectKind::Spotlight => {
                    for light in self.substrate.spotlights.values_mut() {
                        light.mark_logic_membership(false);
                    }
                }
                ObjectKind::ParticleSystem => {
                    for (_, system) in self.substrate.particle_systems.iter_mut() {
                        system.mark_logic_membership(false);
                    }
                }
                ObjectKind::Terrain => self.terrain_objects.clear_logic_memberships(),
                ObjectKind::Projectile => {
                    for (_, projectile) in self.projectiles.iter_mut() {
                        projectile.mark_logic_membership(false);
                    }
                }
                ObjectKind::Wave => {
                    for (_, wave) in self.waves.iter_mut() {
                        wave.mark_logic_membership(false);
                    }
                }
                ObjectKind::Smudge | ObjectKind::Particle => {}
            }
        }
        for id in self.substrate.logic.snapshot() {
            if let Some(kind) = self.classify_object(id).filter(|kind| kind.joins_logic()) {
                self.substrate.logic.note_kind(id, kind);
                Self::set_logic_membership_flag(
                    &mut self.terrain_objects,
                    &mut self.substrate,
                    &mut self.projectiles,
                    &mut self.waves,
                    id,
                    kind,
                    true,
                );
            }
        }
        // Alive, limbo, cell Mark, and death-sequence state are independent
        // serialized facts. Load repair must never derive them from this vector.
    }

    /// Borrow the current canonical dynamic navigation projection.
    pub fn path_grid(&self) -> Option<&PathGrid> {
        self.path_grid.as_deref()
    }

    /// Clone the current projection's `Arc` only to split borrows at a point
    /// of use (the grid read alongside a `&mut self` call). Take it where the
    /// grid is read, never to carry a view across a republish.
    pub fn path_grid_snapshot(&self) -> Option<Arc<PathGrid>> {
        self.path_grid.clone()
    }

    /// Rebuild dynamic structure blockers, zones, and the canonical PathGrid
    /// as one simulation-owned projection.
    pub fn rebuild_dynamic_navigation(&mut self, rules: &RuleSet) -> bool {
        let Some(terrain) = self.resolved_terrain.as_ref() else {
            return false;
        };
        let result = navigation::NavigationCaches {
            zones: &mut self.zone_grid,
            path: &mut self.path_grid,
            overlays: self.overlay_grid.as_ref(),
            playfield_bounds: self.playfield_bounds,
        }
        .rebuild_dynamic(
            terrain,
            self.bridge_state.as_ref(),
            &self.substrate.entities,
            &self.interner,
            rules,
        );
        match result {
            Ok(cells) => self.structure_navigation_cells = cells,
            Err(error) => {
                log::error!("Navigation restoration refused: {error}");
                return false;
            }
        }
        true
    }

    /// Publish marked-structure footprint changes into the path view only.
    /// Native cell occupation marks are read live by the pathfinder, and the
    /// zone map is built from terrain movement classes. Actual PlaceDown and
    /// PlaceUp Recalcs already published any active firestorm wall class.
    fn publish_structure_navigation(&mut self, rules: &RuleSet) {
        let current =
            navigation::structure_movement_cells(&self.substrate.entities, &self.interner, rules);
        let candidates: Vec<_> = current
            .union(&self.structure_navigation_cells)
            .copied()
            .collect();
        self.publish_navigation_cells_with_presence(rules, &candidates, &current);
    }

    /// Finalize mutable overlay identity, passability, and canonical navigation
    /// before the frame hash is latched.
    ///
    /// `dirty_cells` carries mutations that also re-derive CellClass attributes,
    /// mirroring `OverlayClass::Mark`'s `RecalcAttributes` tail; only its
    /// occupied cells produce an upsert. Identity erasures that native performs
    /// *without* a recalc arrive separately through `take_removed_render_cells`
    /// and cross the boundary as removals, since an upsert list cannot express
    /// one.
    pub(super) fn finalize_frame_overlays_and_navigation(
        &mut self,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        navigation_rebuild_requested: bool,
        structures_changed: bool,
    ) -> Vec<OverlayEntry> {
        let mut overlay_updates = Vec::new();
        let mut navigation_cells = Vec::new();
        let overlay_ready = self.resolved_terrain.is_some() && overlay_registry.is_some();
        if overlay_ready && let Some(grid) = self.overlay_grid.as_mut() {
            self.frame_overlay_removals = grid.take_removed_render_cells();
            let dirty_cells = grid.take_dirty_cells();
            navigation_cells = grid.take_synchronous_navigation_cells();

            let terrain = self
                .resolved_terrain
                .as_mut()
                .expect("overlay-ready terrain");
            let registry = overlay_registry.expect("overlay-ready registry");
            let firestorm_zone = FirestormZoneView::new(
                &self.ares_firestorm,
                &self.substrate,
                &self.interner,
                rules,
            );
            for &(rx, ry) in &dirty_cells {
                if recalc_overlay_passability_with_building(
                    grid,
                    terrain,
                    registry,
                    (rx, ry),
                    firestorm_zone.blocks((rx, ry)),
                ) && !navigation_cells.contains(&(rx, ry))
                {
                    navigation_cells.push((rx, ry));
                }
            }
            for (rx, ry) in dirty_cells {
                let cell = grid.cell(rx, ry);
                if let Some(overlay_id) = cell.overlay_id {
                    overlay_updates.push(OverlayEntry {
                        rx,
                        ry,
                        overlay_id,
                        frame: cell.overlay_data,
                    });
                }
            }
        }

        if navigation_rebuild_requested {
            self.recalc_firestorm_zone_cells(rules, &navigation_cells);
            let _ = self.rebuild_dynamic_navigation(rules);
        } else {
            if structures_changed {
                self.publish_structure_navigation(rules);
            }
            self.publish_recalculated_cells(rules, &navigation_cells);
        }
        overlay_updates
    }

    /// Publish completed structure/overlay/terrain Recalcs (`CellClass::RecalcAttributes`
    /// @ `0x0047D2B0`) through the one-cell navigation owner. Zone IDs are not
    /// touched: gamemd's non-wall Mark (`0x005FC570`) and ReduceTiberium
    /// (`0x00480A80`) run no zone helper, and the wall, sale and terrain owners
    /// run theirs themselves.
    pub(crate) fn publish_recalculated_cells(&mut self, rules: &RuleSet, cells: &[(u16, u16)]) {
        // During initial map Reveal no navigation views are installed yet;
        // their first full rebuild reads every admitted Mark. Avoid scanning
        // all structures once per placement before that rebuild.
        if cells.is_empty() || (self.path_grid.is_none() && self.zone_grid.is_none()) {
            return;
        }
        let blocked = navigation::structure_blocked_among(
            &self.substrate.entities,
            &self.interner,
            rules,
            cells,
        );
        self.publish_recalculated_cells_with_presence(rules, cells, &blocked);
    }

    /// [`Self::publish_recalculated_cells`] with structure presence already
    /// scanned. A cache that cannot take one cell (mis-sized during a load
    /// transition) is rebuilt whole rather than left stale.
    fn publish_recalculated_cells_with_presence(
        &mut self,
        rules: &RuleSet,
        cells: &[(u16, u16)],
        blocked: &BTreeSet<(u16, u16)>,
    ) {
        self.recalc_firestorm_zone_cells(rules, cells);
        self.publish_navigation_cells_with_presence(rules, cells, blocked);
    }

    fn recalc_firestorm_zone_cells(&mut self, rules: &RuleSet, cells: &[(u16, u16)]) {
        // PlaceDown/PlaceUp (`0x005683C0`/`0x005687F0`) and overlay or terrain
        // updates publish the current ground list's class before zone repair.
        if self.ares_firestorm.any_active()
            && let Some(terrain) = self.resolved_terrain.as_mut()
        {
            for &cell in cells {
                let impassable = self.ares_firestorm.blocks_zone_cell(
                    &self.substrate.occupancy,
                    &self.substrate.entities,
                    &self.interner,
                    rules,
                    cell,
                );
                terrain.recalc_zone_type_with_buildings_at(cell, impassable);
            }
        }
    }

    fn publish_navigation_cells_with_presence(
        &mut self,
        rules: &RuleSet,
        cells: &[(u16, u16)],
        blocked: &BTreeSet<(u16, u16)>,
    ) {
        let Some(terrain) = self.resolved_terrain.as_ref() else {
            return;
        };
        let mut caches = navigation::NavigationCaches {
            zones: &mut self.zone_grid,
            path: &mut self.path_grid,
            overlays: self.overlay_grid.as_ref(),
            playfield_bounds: self.playfield_bounds,
        };
        let published = cells.iter().try_for_each(|&coord| {
            caches.publish_recalculated_cell_with_presence(terrain, coord, blocked.contains(&coord))
        });
        if let Err(error) = published {
            log::warn!("Recalc navigation publication fell back to a rebuild: {error}");
            let _ = self.rebuild_dynamic_navigation(rules);
            return;
        }
        for &coord in cells {
            if blocked.contains(&coord) {
                self.structure_navigation_cells.insert(coord);
            } else {
                self.structure_navigation_cells.remove(&coord);
            }
        }
    }

    /// Rebuild the zone connectivity map from the current PathGrid. Call after
    /// the PathGrid has been rebuilt so that zones reflect the latest
    /// walkability state. Unchanged cells and classes keep the current zones.
    #[cfg(any(test, feature = "test-support"))]
    pub fn rebuild_zone_grid(&mut self, path_grid: &PathGrid) {
        let Some(terrain) = self.resolved_terrain.as_ref() else {
            return;
        };
        navigation::NavigationCaches {
            zones: &mut self.zone_grid,
            path: &mut self.path_grid,
            overlays: self.overlay_grid.as_ref(),
            playfield_bounds: self.playfield_bounds,
        }
        .rebuild_zones(
            Arc::new(path_grid.clone()),
            terrain,
            self.bridge_state.as_ref(),
        );
    }

    /// Rebuild without `rebuild_zones`' reuse of unchanged inputs. Reduced zone
    /// type can change while boolean walkability stays identical (notably a
    /// live OccupationBits=0 terrain object changing Building to Ground).
    ///
    /// Publishes `path_grid` as it is.
    pub(super) fn rebuild_zone_grid_full(&mut self, path_grid: Arc<PathGrid>) {
        let Some(terrain) = self.resolved_terrain.as_ref() else {
            return;
        };
        navigation::NavigationCaches {
            zones: &mut self.zone_grid,
            path: &mut self.path_grid,
            overlays: self.overlay_grid.as_ref(),
            playfield_bounds: self.playfield_bounds,
        }
        .rebuild_zones_full(path_grid, terrain, self.bridge_state.as_ref());
    }

    /// Consume terrain/overlay receipts before the next path reader. Overlay
    /// receipts publish their Recalc only. A destroyed terrain object follows
    /// `TerrainClass::Limbo` @ `0x0071C930`: Recalc (`0x0071CA2A`), then
    /// AssignOrphanedCellZone (`0x0071CA42`) and the local graph update
    /// (`0x0071CA51`) outside the ScenarioInit scope. Production Terrain
    /// Limbo publishes synchronously before the following receiver.
    pub(super) fn finish_terrain_navigation_changes(
        &mut self,
        rules: &RuleSet,
        terrain_changed_cells: &[(u16, u16)],
    ) {
        let overlay_cells = self
            .overlay_grid
            .as_mut()
            .map(|grid| grid.take_synchronous_navigation_cells())
            .unwrap_or_default();
        let all_cells: Vec<_> = overlay_cells
            .iter()
            .chain(terrain_changed_cells)
            .copied()
            // Dummy Recalc47D2B8 has no path publication or structure scan.
            // Test allocation without performing the ordered lookup early.
            .filter_map(|(x, y)| {
                self.resolved_terrain.as_ref().map_or(Some((x, y)), |grid| {
                    let index = grid.native_fixed_cell_index(x as i16, y as i16)?;
                    let (x, y) = grid
                        .native_cell_coord(crate::map::cell_index::NativeCellIdentity::Real(index));
                    Some((x as u16, y as u16))
                })
            })
            .collect();
        let blocked = navigation::structure_blocked_among(
            &self.substrate.entities,
            &self.interner,
            rules,
            &all_cells,
        );
        self.publish_recalculated_cells_with_presence(rules, &overlay_cells, &blocked);
        // Each Limbo's AssignOrphaned sees later deaths' cells unpublished.
        for &cell in terrain_changed_cells {
            // TerrainLimbo71C9F9..71CA2A reaches this suffix even when
            // already InLimbo. Its canonical lookup must stamp the shared
            // Dummy, whose Recalc47D2B0 immediately returns (47D2B8).
            // Resize565C10's allocation diamond excludes cell0, so the
            // never-Unlimbo'd zero constructor has no real-cell Recalc work.
            let recalc_cell = self
                .resolved_terrain
                .as_ref()
                .map_or(Some(cell), |terrain| {
                    match terrain.native_cell_identity((cell.0 as i16, cell.1 as i16)) {
                        crate::map::cell_index::NativeCellIdentity::Real(index) => {
                            let (x, y) = terrain.native_cell_coord(
                                crate::map::cell_index::NativeCellIdentity::Real(index),
                            );
                            Some((x as u16, y as u16))
                        }
                        crate::map::cell_index::NativeCellIdentity::Dummy => None,
                    }
                });
            let initializing = self.object_placement_scope_active();
            if let Some(recalc_cell) = recalc_cell {
                // Recalc71CA23 uses the allocated Cell receiver's coordinate.
                // Zone/hierarchy71CA42/51 instead retain the packed Location;
                // their own index/geometry is not the Map's fixed512 alias.
                self.publish_recalculated_cells_with_presence(rules, &[recalc_cell], &blocked);
            }
            // 71CA2F..71CA60 gates zone/hierarchy only on ScenarioInit.
            // A Map-Dummy can select a real retained zone record because
            // 56D460 uses the separate Size-derived stride and clamp.
            if !initializing
                && let (Some(terrain), Some(path)) =
                    (self.resolved_terrain.as_ref(), self.path_grid.as_deref())
            {
                repair_zone_after_recalc(
                    &mut self.zone_grid,
                    path,
                    terrain,
                    self.bridge_state.as_ref(),
                    self.playfield_bounds,
                    cell,
                    ZoneRepairKind::AssignOrphaned,
                );
            }
            // Zero's retained zone class7 returns at56D4E8..56D51B;
            // hierarchy58456A..584571 independently rejects its playfield.
            // Do not turn absent Dummy cache input into a global rebuild.
            if !initializing {
                // 71CA2F..71CA60 gates zone repair and radar6551C0 on
                // ScenarioInit==0, independently of the InLimbo prefix.
                self.dirty_cells.mark_radar([cell]);
            }
        }
    }
    /// One Recalc'd cell's navigation publication, then
    /// AssignOrphanedCellZone (`0x0056D460`) and the local graph update
    /// (`0x00584550`): the per-cell tail `TerrainClass::Limbo` and
    /// `BuildingClass::AfterDestruction` share. `blocked` is the structure
    /// presence already scanned for the caller's cells, or calculated here for
    /// a single native caller such as Anim::Start424EE8/424EF3.
    pub(crate) fn publish_cell_and_assign_orphaned_zone(
        &mut self,
        rules: &RuleSet,
        cell: (u16, u16),
        blocked: Option<&BTreeSet<(u16, u16)>>,
    ) {
        let owned_blocked;
        let blocked = match blocked {
            Some(blocked) => blocked,
            None => {
                owned_blocked = navigation::structure_blocked_among(
                    &self.substrate.entities,
                    &self.interner,
                    rules,
                    &[cell],
                );
                &owned_blocked
            }
        };
        self.publish_recalculated_cells_with_presence(rules, &[cell], blocked);
        if let (Some(terrain), Some(path)) =
            (self.resolved_terrain.as_ref(), self.path_grid.as_deref())
        {
            repair_zone_after_recalc(
                &mut self.zone_grid,
                path,
                terrain,
                self.bridge_state.as_ref(),
                self.playfield_bounds,
                cell,
                ZoneRepairKind::AssignOrphaned,
            );
        }
    }

    /// The borrow-split wall observer over this Simulation's live owners.
    pub(crate) fn with_wall_host<R>(
        &mut self,
        rules: &RuleSet,
        apply: impl FnOnce(
            &mut OverlayGrid,
            &mut ResolvedTerrainGrid,
            &mut SimulationWallRuntimeHost<'_>,
        ) -> R,
    ) -> Option<R> {
        let grid = self.overlay_grid.as_mut()?;
        let terrain = self.resolved_terrain.as_mut()?;
        let mut host = SimulationWallRuntimeHost {
            wall_gates: WallGateConnections::new(&self.substrate, &self.interner, rules),
            firestorm_zone: FirestormZoneView::new(
                &self.ares_firestorm,
                &self.substrate,
                &self.interner,
                rules,
            ),
            dirty_cells: &mut self.dirty_cells,
            zone_grid: &mut self.zone_grid,
            path_grid: &mut self.path_grid,
            bridge_state: self.bridge_state.as_ref(),
            playfield_bounds: self.playfield_bounds,
        };
        Some(apply(grid, terrain, &mut host))
    }

    /// BuildingClass::Unlimbo 0x0044104D..0x0044115E and BuildingClass::Limbo
    /// 0x00445B81..0x00445C73 call CellClass::PostDestructionWallCleanup
    /// at the two fixed endpoints of each applicable gate orientation.
    pub(crate) fn refresh_gate_wall_connections(&mut self, id: u64, rules: &RuleSet) {
        let Some(gate) = self
            .substrate
            .entities
            .get(id)
            .filter(|entity| entity.category() == EntityCategory::Structure)
        else {
            return;
        };
        let x = i32::from(gate.position.rx());
        let y = i32::from(gate.position.ry());
        let type_id = gate.type_ref();
        let gate_types = &rules.general.building_types;
        let matches_type = |names: [&Option<String>; 2]| {
            names
                .into_iter()
                .flatten()
                .any(|name| self.interner.get(name) == Some(type_id))
        };
        let east_west = matches_type([&gate_types.gdi_gate_one, &gate_types.nod_gate_one]);
        let north_south = matches_type([&gate_types.gdi_gate_two, &gate_types.nod_gate_two]);
        if !east_west && !north_south {
            return;
        }
        let registry = rules.overlay_registry();
        self.with_wall_host(rules, |grid, terrain, host| {
            let mut refresh = |anchor| {
                refresh_wall_neighborhood_with_host(
                    grid,
                    registry,
                    Some(terrain),
                    anchor,
                    Some(host),
                );
            };
            if east_west {
                refresh((x - 1, y));
                refresh((x + 3, y));
            }
            if north_south {
                refresh((x, y - 1));
                refresh((x, y + 3));
            }
        });
    }

    /// Direct Cell480CB0 caller, retaining its receiver across synchronous
    /// callbacks. Unit73B056 uses the same transaction host as AreaDamage;
    /// the latter's additional outer detach belongs to its own caller.
    pub(crate) fn apply_wall_damage(
        &mut self,
        target: crate::sim::overlay_grid::WallPointerTarget,
        damage: i32,
        overlay_registry: &crate::rules::overlay_types::OverlayTypeRegistry,
        rules: &RuleSet,
    ) {
        let Some(grid) = self.overlay_grid.as_mut() else {
            return;
        };
        let mut host = SimulationWallRuntimeHost {
            wall_gates: WallGateConnections::new(&self.substrate, &self.interner, rules),
            firestorm_zone: FirestormZoneView::new(
                &self.ares_firestorm,
                &self.substrate,
                &self.interner,
                rules,
            ),
            dirty_cells: &mut self.dirty_cells,
            zone_grid: &mut self.zone_grid,
            path_grid: &mut self.path_grid,
            bridge_state: self.bridge_state.as_ref(),
            playfield_bounds: self.playfield_bounds,
        };
        let _ = damage_wall_overlay_with_runtime_host(
            grid,
            overlay_registry,
            self.resolved_terrain.as_mut(),
            target,
            damage,
            &mut self.scenario_rng,
            Some(&mut host),
        );
    }
}
