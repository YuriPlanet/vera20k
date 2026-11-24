//! The `Simulation` side of loading a snapshot: Ares-state admission, the
//! validated post-load restore transaction that re-registers objects and
//! rebuilds skipped indexes, and the map-authority restore for snapshot and
//! player-file loads.

use std::collections::BTreeSet;

use super::object_references::{
    RestoredObjectIndex, restore_object_references, validate_passenger_size_tables,
};
use super::{SnapshotMapRestoreOutput, SnapshotRestoreError, SnapshotTerrainAssetPreparation};
use crate::sim::world::Simulation;

impl Simulation {
    /// BuildingLoad453ECF invokes NoInit43B680 after the saved global Frame
    /// has loaded. Reconstruct its independent repair Stage at the player-file
    /// boundary, preserving mission state, radio links and internal continuations.
    pub(super) fn reconstruct_building_repair_progress_after_player_load(&mut self) {
        let frame = self.session.binary_frame() as i32;
        for entity in self.substrate.entities.values_mut() {
            entity
                .mission_leaf
                .reconstruct_building_repair_progress_after_player_load(frame);
        }
    }

    /// Saved Ares state (a delivery base override, an attached superweapon,
    /// a pending unit delivery, a paid superweapon selection) loads only into
    /// a game Ares plays in; the player load checks it with the loading
    /// game's switch before restoring.
    pub(crate) fn validate_ares_state_for_load(
        &self,
        ares: bool,
    ) -> Result<(), SnapshotRestoreError> {
        for (_, entity) in self.substrate.entities.iter_sorted() {
            entity
                .validate_ares_compatibility(ares)
                .map_err(SnapshotRestoreError::InvalidUnitDelivery)?;
        }
        self.unit_deliveries
            .validate_ares_compatibility(ares)
            .map_err(SnapshotRestoreError::InvalidUnitDelivery)?;
        self.validate_paid_super_weapon_selections_for_load(ares)
            .map_err(SnapshotRestoreError::InvalidPaidSuperWeaponSelection)?;
        Ok(())
    }

    /// Every interned id the saved Simulation holds must name an entry of the
    /// saved interner: both come from the file, and `StringInterner::resolve`
    /// panics on an id it never produced. A save that fails this is refused
    /// before any restore step reads one. Objects, Houses and bullets are
    /// checked first, so their refusal names the object; the walk over the
    /// whole saved state ([`super::saved_ids`]) then covers every other
    /// field (teams and scripts, superweapons, production, House state and
    /// the rest) and names the saved struct and field.
    pub(super) fn validate_saved_interned_ids(&self) -> Result<(), SnapshotRestoreError> {
        let check = |registry, object_id, field, id: crate::sim::intern::InternedId| {
            self.interner
                .try_resolve(id)
                .map(drop)
                .ok_or(SnapshotRestoreError::UnknownInternedId {
                    registry,
                    object_id,
                    field,
                    id: id.index(),
                })
        };
        for (object_id, entity) in self.substrate.entities.iter_sorted() {
            check("object", object_id, "type", entity.type_ref())?;
            check("object", object_id, "owner", entity.owner())?;
        }
        for (&key, house) in &self.houses {
            let object_id = u64::from(key.index());
            check("House", object_id, "key", key)?;
            check("House", object_id, "type", house.house_type_id())?;
        }
        for (&bullet_id, bullet) in self.projectiles.iter() {
            let payload = bullet.payload();
            check("bullet", bullet_id, "warhead", payload.warhead)?;
            if let Some(projectile_type) = payload.projectile_type() {
                check("bullet", bullet_id, "projectile type", projectile_type)?;
            }
            if let Some(weapon) = payload.installed_weapon() {
                check("bullet", bullet_id, "weapon", weapon)?;
            }
        }
        super::saved_ids::check_saved_interned_ids(self, self.interner.len())?;
        Ok(())
    }

    /// Custom sessions resume authoritative state instead of running the
    /// resets that mimic a stock player load.
    pub fn retains_snapshot_continuation(&self) -> bool {
        !self.session.has_stock_rule_policy() || self.session.has_custom_control_grants()
    }

    /// Validate and re-register a deserialized simulation before it can resume.
    ///
    /// Native load registers old identities and pointer slots, resolves them,
    /// then restores global active/cell membership. Rust stores stable IDs
    /// directly, so the equivalent transaction builds one global namespace,
    /// rejects any unmatched modeled native pointer, cleans only deliberate
    /// weak/derived identities, and reconstructs skipped indexes in dependency
    /// order.
    pub(crate) fn restore_after_snapshot_load(&mut self) -> Result<(), SnapshotRestoreError> {
        let retain_continuation = self.retains_snapshot_continuation();
        for (_, entity) in self.substrate.entities.iter_sorted() {
            if !entity.seldom.type_facts.foundation().valid() {
                return Err(SnapshotRestoreError::InvalidFoundation {
                    object_id: entity.stable_id(),
                });
            }
        }
        self.validate_paid_super_weapon_selections()
            .map_err(SnapshotRestoreError::InvalidPaidSuperWeaponSelection)?;
        self.super_weapons
            .validate_ares_launch_history()
            .map_err(SnapshotRestoreError::InvalidSuperWeaponLaunchHistory)?;
        self.unit_deliveries
            .validate(self)
            .map_err(SnapshotRestoreError::InvalidUnitDelivery)?;
        self.validate_restored_spotlights()
            .map_err(
                |(object_id, reason)| SnapshotRestoreError::InvalidSpotlightState {
                    object_id,
                    reason,
                },
            )?;
        self.projectiles
            .validate_scalable_runtime(&self.interner)
            .map_err(SnapshotRestoreError::InvalidScalableProjectileRuntime)?;
        self.radiation
            .validate_site_order()
            .map_err(SnapshotRestoreError::InvalidRadiationSiteOrder)?;
        self.validate_planning_references()
            .map_err(|reason| SnapshotRestoreError::InvalidPlanningReferences { reason })?;
        self.team_script_vm
            .validate_restored_task_forces()
            .map_err(|reason| SnapshotRestoreError::InvalidTeamState { reason })?;
        if let Some(owner) = self.session.current_house()
            && !self.houses.contains_key(&owner)
        {
            return Err(SnapshotRestoreError::InvalidCurrentHouse { owner });
        }
        if self.session.outcome_request().is_some() && self.session.current_house().is_none() {
            return Err(SnapshotRestoreError::InvalidFrameContinuation {
                reason: "scenario result request has no current House",
            });
        }
        self.validate_frame_continuation_for_restore()
            .map_err(|reason| SnapshotRestoreError::InvalidFrameContinuation { reason })?;
        if self
            .session
            .observer_houses()
            .iter()
            .any(|observer| !self.houses.contains_key(observer))
        {
            return Err(SnapshotRestoreError::InvalidMultiplayerSession {
                reason: "observer House is absent",
            });
        }
        if self.session.execution_topology().is_lockstep()
            && self.session.network_max_ahead_frames().is_none()
        {
            return Err(SnapshotRestoreError::InvalidMultiplayerSession {
                reason: "network timing is absent",
            });
        }
        if self.session.execution_topology().is_lockstep()
            && self.session.rule_policy().counts_player_control_as_human()
        {
            return Err(SnapshotRestoreError::InvalidMultiplayerSession {
                reason: "lockstep rules cannot read a local PlayerControl flag",
            });
        }
        self.session
            .validate_restored_control_grants(&self.houses)?;
        let scenario_names_admitted = self
            .houses
            .values()
            .any(|house| house.native_scenario_name().is_some());
        // VERA-internal: the dying mark is the SHP vehicle/aircraft death and
        // UnInit's. A live infantryman with it would take the vehicle death
        // path; his corpse is an ordinary IsAlive object at Health 0.
        for (object_id, entity) in self.substrate.entities.iter_sorted() {
            if entity.category() == crate::map::entities::EntityCategory::Infantry
                && entity.death.is_dying()
                && entity.lifecycle.object_alive()
            {
                return Err(SnapshotRestoreError::InvalidInfantryDyingMark { object_id });
            }
        }
        for (&owner, house) in &self.houses {
            house.validate_scenario_name().map_err(|reason| {
                SnapshotRestoreError::InvalidHouseScenarioName { owner, reason }
            })?;
            if scenario_names_admitted && house.native_scenario_name().is_none() {
                return Err(SnapshotRestoreError::InvalidHouseScenarioName {
                    owner,
                    reason: "admitted scenario requires a name for every registered House",
                });
            }
            house.validate_production_history().map_err(|reason| {
                SnapshotRestoreError::InvalidHouseProductionHistory { owner, reason }
            })?;
            if house.is_to_die() && (house.has_won() || house.has_lost()) {
                return Err(SnapshotRestoreError::InvalidHouseOutcomeState {
                    owner,
                    reason: "Flag_To_Die conflicts with a terminal result",
                });
            }
            // FlagWin4FC9EE refuses Lost, and FlagLose4FCBDE clears Won.
            // House4F8657 also clears Won after its wait, independently of
            // the sticky process result requests. A later Action2 may change
            // the House flags and restart +298 after victory was admitted.
            if house.has_won() && house.has_lost() {
                return Err(SnapshotRestoreError::InvalidHouseOutcomeState {
                    owner,
                    reason: "Won and Lost flags conflict",
                });
            }
        }

        let (index, identities) = RestoredObjectIndex::build(self)?;
        let seen_logic = self
            .substrate
            .validate_restored_counters_and_logic(index.highest_id, &identities)?;
        let pending_delete_ids = self
            .substrate
            .pending_delete
            .object_tickets()
            .collect::<BTreeSet<_>>();
        for (object_id, _, alive, in_limbo) in self.substrate.particle_systems.detached_particles()
        {
            if seen_logic.contains(&object_id)
                || (in_limbo && self.display_layers().layer_of(object_id).is_some())
                || (!alive && !in_limbo)
            {
                return Err(SnapshotRestoreError::InvalidDetachedParticleState {
                    object_id,
                    reason: "detached body remains in Logic, limbo body remains in Display, or dead body is not in limbo",
                });
            }
            if !alive && !pending_delete_ids.contains(&object_id) {
                return Err(SnapshotRestoreError::MissingDeferredDeleteIdentity {
                    registry: "ParticleSystemStore.detached",
                    object_id,
                });
            }
        }
        for ticket in self.substrate.pending_delete.retained_tickets() {
            use crate::sim::world::DeferredDeleteTicket;
            let (registry, identity, resolves) = match ticket {
                DeferredDeleteTicket::Object(id) => ("Object", id, identities.contains_key(&id)),
                DeferredDeleteTicket::Overlay(id) => {
                    ("Overlay", id, self.load_objects.has_overlay(id))
                }
                DeferredDeleteTicket::DiskLaser(id) => ("DiskLaser", id, self.has_disk_laser(id)),
                DeferredDeleteTicket::Tag(id) => {
                    ("Tag", id as u64, self.trigger_runtime.has_live_tag(id))
                }
                DeferredDeleteTicket::Trigger(id) => (
                    "Trigger",
                    id as u64,
                    self.trigger_runtime.has_live_trigger(id),
                ),
            };
            if !resolves {
                return Err(SnapshotRestoreError::UnresolvedDeferredDeleteTicket {
                    registry,
                    identity,
                });
            }
        }
        if let Some(grid) = &self.smudge_grid {
            for (object_id, object) in grid.objects() {
                if object.object_alive() || !object.in_limbo() {
                    return Err(SnapshotRestoreError::InvalidSmudgeLifetime { object_id });
                }
                if seen_logic.contains(&object_id) {
                    return Err(SnapshotRestoreError::InactiveLogicIdentity {
                        registry: "SmudgeGrid",
                        object_id,
                    });
                }
                if !pending_delete_ids.contains(&object_id) {
                    return Err(SnapshotRestoreError::MissingDeferredDeleteIdentity {
                        registry: "SmudgeGrid",
                        object_id,
                    });
                }
            }
        }
        let cell_grid = self.cell_grid();
        self.terrain_objects.configure_cell_grid(cell_grid);
        self.houses.configure_cell_grid(cell_grid);
        self.terrain_objects
            .validate_membership()
            .map_err(|reason| SnapshotRestoreError::InvalidCellMembership { reason })?;
        for terrain in self.terrain_objects.objects().values() {
            let in_logic = seen_logic.contains(&terrain.stable_id());
            let pending_delete = pending_delete_ids.contains(&terrain.stable_id());
            if !terrain.is_live() && in_logic {
                return Err(SnapshotRestoreError::InactiveLogicIdentity {
                    registry: "TerrainObjectStore",
                    object_id: terrain.stable_id(),
                });
            }
            if terrain.object_alive() && pending_delete {
                return Err(SnapshotRestoreError::DeferredDeleteLogicIdentity {
                    registry: "TerrainObjectStore",
                    object_id: terrain.stable_id(),
                });
            }
            // ObjectCtor5F3930 starts Alive even while InLimbo. A zero-cell
            // TerrainCtor71BC34 skips Unlimbo without scheduling deletion;
            // ObjectUnInit5F6625, rather than Limbo, clears Alive.
            if !terrain.object_alive() && !pending_delete {
                return Err(SnapshotRestoreError::MissingDeferredDeleteIdentity {
                    registry: "TerrainObjectStore",
                    object_id: terrain.stable_id(),
                });
            }
        }
        for (&object_id, _) in self.projectiles.iter() {
            let in_logic = seen_logic.contains(&object_id);
            let pending_delete = pending_delete_ids.contains(&object_id);
            if !in_logic && !pending_delete {
                return Err(SnapshotRestoreError::MissingRequiredLogicIdentity {
                    registry: "ProjectileStore",
                    object_id,
                });
            }
            if in_logic && pending_delete {
                return Err(SnapshotRestoreError::DeferredDeleteLogicIdentity {
                    registry: "ProjectileStore",
                    object_id,
                });
            }
        }
        for (&object_id, _) in self.waves.iter() {
            let in_logic = seen_logic.contains(&object_id);
            let pending_delete = pending_delete_ids.contains(&object_id);
            if !in_logic && !pending_delete {
                return Err(SnapshotRestoreError::MissingRequiredLogicIdentity {
                    registry: "WaveStore",
                    object_id,
                });
            }
            if in_logic && pending_delete {
                return Err(SnapshotRestoreError::DeferredDeleteLogicIdentity {
                    registry: "WaveStore",
                    object_id,
                });
            }
        }

        validate_passenger_size_tables(self)?;
        // Concrete Unit74451C/Aircraft41B4DC/Infantry521A0C/Building453ECF
        // Load reconstructs Techno's three empty vectors through NoInit
        //6F43AB..6F4410. Their raw headers have no element swizzles; discard
        // them before validating strong references, retaining real TarCom.
        if !retain_continuation {
            // Each reset writes only its own object, so visit order is free.
            for entity in self.substrate.entities.values_mut() {
                entity.clear_distributed_fire_after_load();
            }
        }
        restore_object_references(self, &identities)?;

        // EntityStore already restored its indexes during Deserialize (Clone
        // retains them too). Reference restoration above changes no indexed identity.
        // Continue with Logic slots (including ParticleSystem). Actual CellClass
        // list heads/order were serialized, like483C10/4839F0: validate references
        // without requerying geometry before map authority is reattached.
        let frame = self.session.binary_frame();
        if !retain_continuation {
            self.projectiles.restart_arm_timers_after_load(frame as i32);
            self.projectiles.restore_scalable_after_load();
            // RadSiteClass::Load65B3D0 restarts its level/light clocks;
            // the saved field and source stay intact until ordinary RadSite AI.
            self.radiation.restart_timers_after_load(frame);
            for id in self.substrate.entities.keys_sorted() {
                // FootClass::Load 0x004DB60D..0x004DB624 clears load-time
                // audio. An exact continuation keeps the logical latch and
                // countdown because they control the next Main RNG draw.
                self.substrate.restore_move_sound_state_after_load(id);
                // ParasiteClass Load6295DB..6295F3 leaves a bite due and
                // drops suppression on a stock player load.
                if let Some(entity) = self.substrate.entities.get_mut(id) {
                    entity.parasite.restart_timers_after_load(frame);
                }
            }
        }
        self.rebuild_logic_membership();
        self.substrate.rebuild_building_anim_slot_indices();
        Self::rebuild_bomb_carriers(&self.substrate, &mut self.bombs);
        if !retain_continuation {
            // The kamikaze tracker's Load (`0x0054E7B0`) follows its Clear.
            self.kamikaze.restart_after_load(frame);
        }
        self.substrate
            .occupancy
            .restore_memberships(&self.substrate.entities)
            .map_err(|reason| SnapshotRestoreError::InvalidCellMembership { reason })?;
        Ok(())
    }

    /// Reproject serialized overlay and bridge authority onto the fresh
    /// map-derived terrain cache, then publish canonical navigation before the
    /// restored simulation can run another frame.
    ///
    /// The full row-major sweep is required because OverlayGrid's dirty queues
    /// are transient: a saved cell may have been cleared since map load, so an
    /// occupied-only replay would leave the original map overlay's passability
    /// behind. Bridge cell state needs no separate pass: its flags return
    /// through the dynamic terrain cells above, and its overlay identity and
    /// state byte through OverlayGrid.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn restore_map_authority_after_snapshot_load(
        &mut self,
        rules: &crate::rules::ruleset::RuleSet,
        overlay_registry: &crate::rules::overlay_types::OverlayTypeRegistry,
        prepare_terrain_assets: Option<&mut SnapshotTerrainAssetPreparation<'_>>,
    ) -> Result<SnapshotMapRestoreOutput, SnapshotRestoreError> {
        self.restore_map_authority_after_load(rules, overlay_registry, prepare_terrain_assets, None)
    }

    /// Player-file CellLoad4839F0 / Map no-init565190 crate resets.
    /// Internal simulation continuations and Ares games keep their saved
    /// crates.
    pub(crate) fn restore_map_authority_after_user_load(
        &mut self,
        rules: &crate::rules::ruleset::RuleSet,
        overlay_registry: &crate::rules::overlay_types::OverlayTypeRegistry,
        prepare_terrain_assets: Option<&mut SnapshotTerrainAssetPreparation<'_>>,
        saved_land: crate::map::resolved_terrain::SavedCellLandValues,
    ) -> Result<SnapshotMapRestoreOutput, SnapshotRestoreError> {
        self.restore_map_authority_after_load(
            rules,
            overlay_registry,
            prepare_terrain_assets,
            Some(saved_land),
        )
    }

    fn restore_map_authority_after_load(
        &mut self,
        rules: &crate::rules::ruleset::RuleSet,
        overlay_registry: &crate::rules::overlay_types::OverlayTypeRegistry,
        prepare_terrain_assets: Option<&mut SnapshotTerrainAssetPreparation<'_>>,
        saved_land: Option<crate::map::resolved_terrain::SavedCellLandValues>,
    ) -> Result<SnapshotMapRestoreOutput, SnapshotRestoreError> {
        self.validate_type_tech_levels(rules)?;
        for (id, name, _) in self.voxel_type_damage_identity_entries() {
            if rules.voxel_anim_type_id_by_name(name) != Some(id)
                || rules.voxel_anim_type(id).name != name
            {
                return Err(SnapshotRestoreError::InvalidVoxelTypeDamage);
            }
        }
        for (&object_id, debris) in self.substrate.voxel_anims.iter() {
            if debris.type_id().0 as usize >= rules.voxel_anim_type_count() {
                return Err(SnapshotRestoreError::InvalidVoxelAnimType { object_id });
            }
        }
        // TerrainLoad71CDA0 preserves the separately saved Display/Logic
        // vectors. Only ObjectUnlimbo5F4FEF..5F5040's eligible spawners need
        // Logic; validate with restored type state without rebuilding order.
        if let Some(grid) = self.resolved_terrain.as_ref() {
            for &(x, y) in self.terrain_objects.cell_index().keys() {
                if grid.native_fixed_cell_index(x as i16, y as i16).is_none() {
                    return Err(SnapshotRestoreError::InvalidCellMembership {
                        reason: format!("Terrain Cell ({x}, {y}) has no native allocation"),
                    });
                }
            }
        }
        for terrain in self.terrain_objects.objects().values() {
            if !terrain.is_live() {
                continue;
            }
            // The type id and the interner both come from the save, so an id
            // past the interner is a missing type like an unknown name.
            let type_name = self.interner.try_resolve(terrain.type_ref());
            let terrain_type = type_name
                .and_then(|name| rules.terrain_object_type_case_insensitive(name))
                .ok_or_else(|| SnapshotRestoreError::MissingTerrainType {
                    object_id: terrain.stable_id(),
                    type_name: type_name
                        .map_or_else(|| format!("#{}", terrain.type_ref().index()), str::to_owned),
                })?;
            // TypeLoad71E22D rebinds its retained selector, but only actual
            // live instance admission needs an executable terminated list.
            // Preserve saved Cell heads; never infer them from this check.
            terrain_type.occupied_cell_offsets().map_err(|error| {
                SnapshotRestoreError::InvalidCellMembership {
                    reason: error.to_string(),
                }
            })?;
            if !terrain.in_logic_vector()
                && terrain.joins_logic_on_unlimbo(
                    terrain_type,
                    self.session.rule_policy(),
                    rules.ares_form(),
                )
            {
                return Err(SnapshotRestoreError::MissingRequiredLogicIdentity {
                    registry: "TerrainObjectStore",
                    object_id: terrain.stable_id(),
                });
            }
        }
        let ares = rules.ares_form();
        if ares {
            self.validate_attached_stats_after_restore(rules)
                .map_err(
                    |(object_id, reason)| SnapshotRestoreError::InvalidAttachedStatsState {
                        object_id,
                        reason,
                    },
                )?;
            self.validate_factory_country_references(rules)
                .map_err(|reason| SnapshotRestoreError::InvalidFactoryCountry { reason })?;
        }
        self.validate_disk_lasers_after_restore(rules)
            .map_err(
                |(object_id, reason)| SnapshotRestoreError::InvalidDiskLaserState {
                    object_id,
                    reason,
                },
            )?;
        let (overlay_width, overlay_height, overlay_cell_count, retained_wall_count) = self
            .overlay_grid
            .as_ref()
            .map(|grid| {
                (
                    grid.width(),
                    grid.height(),
                    grid.cell_storage_len(),
                    grid.retained_neighbor_count_storage_len(),
                )
            })
            .ok_or(SnapshotRestoreError::MissingMapAuthorityComponent {
                component: "OverlayGrid",
            })?;
        let expected_overlay_cell_count = overlay_width as usize * overlay_height as usize;
        if overlay_cell_count != expected_overlay_cell_count {
            return Err(SnapshotRestoreError::MapAuthorityCellStorageMismatch {
                expected: expected_overlay_cell_count,
                found: overlay_cell_count,
            });
        }
        // `CellClass+0x122`'s wall contribution is written only by
        // `OverlayClass::Mark` and decremented only by an explicit removal;
        // nothing in gamemd rebuilds it from final wall identities, so the
        // saved plane must cover every cell.
        if retained_wall_count != expected_overlay_cell_count {
            return Err(SnapshotRestoreError::RetainedWallNeighborStorageMismatch {
                expected: expected_overlay_cell_count,
                found: retained_wall_count,
            });
        }
        let (terrain_width, terrain_height) = self
            .resolved_terrain
            .as_ref()
            .map(|terrain| (terrain.width(), terrain.height()))
            .ok_or(SnapshotRestoreError::MissingMapAuthorityComponent {
                component: "ResolvedTerrainGrid",
            })?;
        if (overlay_width, overlay_height) != (terrain_width, terrain_height) {
            return Err(SnapshotRestoreError::MapAuthorityDimensionMismatch {
                overlay_width,
                overlay_height,
                terrain_width,
                terrain_height,
            });
        }

        if let Some(saved_land) = saved_land.as_ref() {
            // MouseLoad restores Map+68/raw rows before LoadContent67E8CD's
            // hierarchy pass. A user file cannot invent that saved topology
            // from the Cell+4C values which CellLoad has just reset to zero.
            let zones = self.zone_grid.as_ref().ok_or(
                SnapshotRestoreError::MissingMapAuthorityComponent {
                    component: "ZoneGrid",
                },
            )?;
            if !zones.is_native_load_pending() {
                return Err(SnapshotRestoreError::NavigationAuthorityPublicationFailed);
            }
            // Ares skips both resets, so a loaded Ares game keeps its crates:
            // CellClass_Load_Crates (hook 0x00483BF1 -> 0x00483BFE, Ares.dll
            // 1006ADE0) and MapClass_CTOR_NoInit_Crates (0x00565215 ->
            // 0x0056522D, 1006AEC0).
            if !self.retains_snapshot_continuation() && !rules.ares() {
                self.reset_crates_after_user_load(saved_land)?;
            }
        }

        // Reproject runtime isometric replacements onto the pristine
        // app-supplied map before overlay, bridge, and zone reconstruction.
        self.dynamic_terrain_cells
            .apply_to(
                self.resolved_terrain
                    .as_mut()
                    .expect("validated terrain cache"),
            )
            .map_err(|(rx, ry)| SnapshotRestoreError::DynamicTerrainCellMissing { rx, ry })?;

        // MouseClass::Load @ 0x005BE657 calls the restored-cell TMP admission
        // pass @ 0x00546DA0 before publishing the loaded map. Admission must
        // see saved live replacements, not the baseline terrain. The optional
        // app callback binds retail assets and can consume only this candidate's
        // Main draws; legacy/headless callers already carry their catalogs.
        if let Some(prepare_terrain_assets) = prepare_terrain_assets {
            let (size_width, _) = self.map_size_diamond().ok_or(
                SnapshotRestoreError::MissingMapAuthorityComponent {
                    component: "Map Size",
                },
            )?;
            prepare_terrain_assets(
                self.resolved_terrain
                    .as_mut()
                    .expect("validated terrain cache"),
                &mut self.main_rng,
                size_width,
            )
            .map_err(|reason| SnapshotRestoreError::TerrainAssetPreparationFailed { reason })?;
        }

        // Native CellClass::Load restores allocated real-cell flag words as
        // values. Do this before derived overlay/bridge reconstruction and do
        // not route through GetCell or SetBridgeDirection: those would stamp
        // the process-global dummy that Resize reconstructs at commit.
        if !self
            .resolved_terrain
            .as_mut()
            .expect("validated terrain cache")
            .restore_real_cell_bridge_flags_0x1180(&self.real_cell_bridge_flags_0x1180)
        {
            return Err(SnapshotRestoreError::RealCellBridgeFlagAuthorityMismatch);
        }

        // The app-supplied terrain template is the immutable load-time map.
        // Action 40's mutable LocalSize is serialized on Simulation, so replay
        // its CellClass-derived cache before overlay and zone reconstruction.
        if let Some(bounds) = self.playfield_bounds() {
            let _ = self
                .resolved_terrain
                .as_mut()
                .expect("validated terrain cache")
                .recalc_playfield_attributes(bounds);
        }

        {
            let overlay_grid = self.overlay_grid.as_mut().expect("validated overlay cache");
            let resolved_terrain = self
                .resolved_terrain
                .as_mut()
                .expect("validated terrain cache");
            for ry in 0..overlay_height {
                for rx in 0..overlay_width {
                    let _ = crate::sim::overlay_grid::recalc_overlay_passability(
                        overlay_grid,
                        resolved_terrain,
                        overlay_registry,
                        rx,
                        ry,
                    );
                }
            }
        }

        if let Some(saved_land) = saved_land {
            self.resolved_terrain
                .as_mut()
                .expect("validated terrain cache")
                .restore_user_loaded_land(&saved_land)
                .map_err(|error| SnapshotRestoreError::InvalidSavedCellLand(error.to_string()))?;
        }

        // Ares 3.0 10014B70 (hook 0x483D94): a powered firestorm section's
        // Impassable class is a Recalc product the template lacks.
        if ares {
            self.ares_firestorm_restore_zone_classes(rules);
        }
        if !self.rebuild_dynamic_navigation(rules) {
            return Err(SnapshotRestoreError::NavigationAuthorityPublicationFailed);
        }

        let occupied_overlays = self
            .overlay_grid
            .as_ref()
            .expect("validated overlay cache")
            .occupied_entries();
        let native_tiberium_stats = if self.retains_snapshot_continuation() {
            crate::sim::ore_growth::NativeTiberiumRebuildStats::default()
        } else {
            self.rebuild_native_tiberium_queues_after_snapshot_load(rules, overlay_registry)?
        };
        // CellTag identities are meaningful only after the canonical Cell
        // allocation is restored. Keep their final owner validation in this
        // shared admission seam, before an app can publish the candidate.
        self.validate_restored_trigger_runtime(rules.ares_form())
            .map_err(|reason| SnapshotRestoreError::InvalidTriggerRuntime { reason })?;
        Ok(SnapshotMapRestoreOutput {
            occupied_overlays,
            native_tiberium_stats,
        })
    }

    /// Replace serialized Tiberium queue stores with their native post-load
    /// cell-derived result. This consumes no RNG and leaves both class timers
    /// due at the restored binary frame.
    ///
    /// gamemd-derived: `TiberiumClass::Load @ 0x00721E80` discards both
    /// dynamic queue stores and calls `InitGrowthQueues_All @ 0x00722D00` then
    /// `InitSpreadQueues_All @ 0x00722240`. Spread admission reads the ground
    /// CellClass object list, never the bridge/AltObject list.
    fn rebuild_native_tiberium_queues_after_snapshot_load(
        &mut self,
        rules: &crate::rules::ruleset::RuleSet,
        overlay_registry: &crate::rules::overlay_types::OverlayTypeRegistry,
    ) -> Result<crate::sim::ore_growth::NativeTiberiumRebuildStats, SnapshotRestoreError> {
        let overlay_grid = self.overlay_grid.as_ref().ok_or(
            SnapshotRestoreError::MissingMapAuthorityComponent {
                component: "OverlayGrid",
            },
        )?;
        let resolved_terrain = self.resolved_terrain.as_ref().ok_or(
            SnapshotRestoreError::MissingMapAuthorityComponent {
                component: "ResolvedTerrainGrid",
            },
        )?;
        let mut source_object_cells: BTreeSet<(u16, u16)> =
            self.terrain_objects.cell_index().keys().copied().collect();
        source_object_cells.extend(
            self.substrate
                .occupancy
                .occupied_cells_on_layer(crate::sim::movement::locomotor::MovementLayer::Ground),
        );

        let grows = self.ore_growth_config.grows();
        let spreads = self.ore_growth_config.spreads();
        let current_frame = self.session.binary_frame();
        // The stores are sized and walked from the native `MapRect` (`[Map]
        // Size`), never the cell-array extent in `session.map_*`. The rect the
        // fresh load rebuilt with is serialized; a zero rect (no rebuild ever
        // ran) falls back to the retained MapClass Size dimensions.
        let mut native_rect = self.ore_growth_state.native_rect();
        if native_rect == (0, 0)
            && let (Some(bounds), Some(height)) =
                (self.playfield_bounds(), self.playfield_size_height())
        {
            native_rect = (
                u16::try_from(bounds.base).unwrap_or(0),
                u16::try_from(height).unwrap_or(0),
            );
        }
        Ok(self
            .ore_growth_state
            .rebuild_native_tiberium_queues_from_overlays(
                overlay_grid,
                overlay_registry,
                &rules.tiberium_types,
                Some(resolved_terrain),
                &source_object_cells,
                grows,
                spreads,
                current_frame,
                native_rect,
            )?)
    }
}
