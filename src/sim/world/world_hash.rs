//! Deterministic state hashing for the Simulation.
//!
//! Produces a reproducible u64 hash over the entire simulation state:
//! tick counter, RNG state, production queues, fog-of-war, entity components.
//! Used for replay verification and desync detection in multiplayer.
//!
//! Dependency rules: same as sim/ (depends on rules/, map/; never render/ui/audio/net).

use std::hash::{Hash, Hasher};

use super::Simulation;
use crate::sim::game_entity::GameEntity;
use crate::sim::overlay_grid::OverlayCell;
use crate::sim::parallel;
use crate::sim::state_hasher::digests;
use crate::util::state_hasher::{self, StateHasher};

/// [`Simulation::state_hash_owners`]: each part's name and digest.
pub type StateHashOwners = [(&'static str, u64); 34];

mod entity;

use entity::hash_entity_owed;

fn hash_projectile_target(
    target: crate::sim::projectile::ProjectileTarget,
    hasher: &mut impl Hasher,
) {
    match target {
        crate::sim::projectile::ProjectileTarget::Entity(id) => {
            0u8.hash(hasher);
            id.hash(hasher);
        }
        crate::sim::projectile::ProjectileTarget::Cell { rx, ry } => {
            1u8.hash(hasher);
            rx.hash(hasher);
            ry.hash(hasher);
        }
        crate::sim::projectile::ProjectileTarget::None => {
            2u8.hash(hasher);
        }
        crate::sim::projectile::ProjectileTarget::DummyCell => {
            3u8.hash(hasher);
        }
        crate::sim::projectile::ProjectileTarget::Terrain(id) => {
            // Bullet+10C retains a distinct Object pointer kind. Keep all
            // established tags stable and never alias Entity(id) to a tree.
            4u8.hash(hasher);
            id.hash(hasher);
        }
    }
}

/// Fold the direct House CRC fields around CurrentIQ in native order.
///
/// gamemd-derived: raw House CRC `0x00502D60..0x0050303F` folds Production at
/// `0x00502E58`, AutocreateAllowed at `0x00502E66`, AITriggersActive at
/// `0x00502E74`, and CurrentIQ at `0x00502E90`; exhaustive census finds no
/// direct AutoBaseBuilding (`House+0x1F3`) feed.
fn hash_house_ai_activation_fields(
    house: &crate::sim::house_state::HouseState,
    hasher: &mut impl Hasher,
) {
    house.ai_activation().production.hash(hasher);
    house.ai_activation().autocreate_allowed.hash(hasher);
    house.ai_activation().ai_triggers_active.hash(hasher);
    house.current_iq().hash(hasher);
}

impl Simulation {
    /// Read-only Rust diagnostic projections through the existing hash owners.
    /// These are attribution receipts, never gameplay/native golden inputs.
    /// Everything [`Self::state_hash`] folds falls under one of these, so two
    /// clients' lists, as a match that never started reports them, name what
    /// differs. `interner` is outside the hash: it tells apart two clients
    /// whose Houses and objects differ only in how names were numbered.
    pub fn state_hash_owners(&self) -> StateHashOwners {
        fn digest(fold: impl FnOnce(&mut StateHasher)) -> u64 {
            let mut hasher = StateHasher::new();
            fold(&mut hasher);
            hasher.finish()
        }
        [
            ("fold_before_fog", digest(|h| self.fold_before_fog(h))),
            ("houses", digest(|h| self.hash_houses(h))),
            (
                "entities",
                digest(|h| {
                    let entities: Vec<_> = self.substrate.entities.values().collect();
                    Self::hash_entities(&self.entity_digests(&entities), h)
                }),
            ),
            (
                "raw_occupation",
                digest(|h| self.substrate.fold_raw_cell_occupation(h)),
            ),
            (
                "cell_memberships",
                digest(|h| self.substrate.occupancy.hash_memberships(h)),
            ),
            (
                "hidden_occupation",
                digest(|h| self.substrate.fold_hidden_occupation(h)),
            ),
            ("air_slots", digest(|h| self.substrate.fold_air_slots(h))),
            (
                "base_reservations",
                digest(|h| self.substrate.fold_base_reservations(h)),
            ),
            (
                "navigation_history",
                digest(|h| {
                    if let Some(zones) = &*self.zone_grid {
                        zones.fold_navigation_history(h);
                    }
                }),
            ),
            // The trigger runtime in its four parts, so a start that failed
            // on it says whether locals, globals or the map's definitions
            // (action operands among them) differed.
            (
                "trigger_globals",
                digest(|h| self.trigger_runtime.hash_globals(h)),
            ),
            (
                "trigger_locals",
                digest(|h| self.trigger_runtime.hash_locals(h)),
            ),
            (
                "trigger_definitions",
                digest(|h| self.trigger_runtime.hash_definitions(h)),
            ),
            (
                "trigger_live",
                digest(|h| self.trigger_runtime.hash_live(h)),
            ),
            (
                "team_script_vm",
                digest(|h| {
                    self.team_script_vm
                        .hash_state(self.session.binary_frame() as i32, h)
                }),
            ),
            (
                "fog_alliances",
                digest(|h| self.hash_fog_and_alliances(&self.fog_digests(), h)),
            ),
            ("power", digest(|h| self.hash_power_states(h))),
            ("planning", digest(|h| self.planning.hash_state(h))),
            ("projectiles", digest(|h| self.hash_projectiles(h))),
            ("scenario_rng", digest(|h| self.scenario_rng.hash_state(h))),
            ("mapgen_rng", digest(|h| self.hash_network_mapgen(h))),
            (
                "native_unique_ids",
                digest(|h| self.native_unique_ids.hash(h)),
            ),
            ("tech_levels", digest(|h| self.hash_type_tech_levels(h))),
            ("playfield", digest(|h| self.hash_playfield_authority(h))),
            (
                "object_order",
                digest(|h| {
                    self.substrate.logic.as_slice().hash(h);
                    self.substrate.display.fold_hash(h);
                }),
            ),
            (
                "game_options",
                digest(|h| self.session.fold_game_options(h)),
            ),
            (
                "bridges",
                digest(|h| {
                    self.hash_bridge_state(h);
                    self.pending_bridge_collapses.hash_state(h);
                    self.real_cell_bridge_flags_0x1180.hash(h);
                }),
            ),
            (
                "terrain_cells",
                digest(|h| {
                    self.dynamic_terrain_cells.hash_simulation(h);
                    self.hash_ice_cracks(h);
                    self.hash_terrain_dent(h);
                }),
            ),
            (
                "burrowing",
                digest(|h| {
                    if let Some(terrain) = &*self.resolved_terrain {
                        terrain.hash_burrowing_permissions(h);
                    }
                }),
            ),
            (
                "map_objects",
                digest(|h| {
                    self.hash_overlay_grid(h);
                    self.hash_crate_authority(h);
                    self.hash_smudge_grid(h);
                    self.hash_radiation(h);
                }),
            ),
            (
                "effects",
                digest(|h| {
                    self.kamikaze.fold_hash(h);
                    self.hash_waves(h);
                    self.disk_lasers.hash_state(h);
                    self.hash_anims(h);
                    self.hash_voxel_anims(h);
                    self.substrate.spotlights.hash_state(h);
                    self.hash_particle_systems(h);
                }),
            ),
            ("super_weapons", digest(|h| self.hash_super_weapons(h))),
            (
                "shared_cell_dummy",
                digest(|h| {
                    let entities: Vec<_> = self.substrate.entities.values().collect();
                    let retained =
                        self.cells_anchor_dummy() || self.entities_target_dummy(&entities);
                    self.hash_shared_cell_dummy(retained, h);
                }),
            ),
            (
                "scenario_identity",
                digest(|h| {
                    self.session.fold_identity(self.houses.order(), h);
                    self.session.pixel_conversion_bounds().hash(h);
                }),
            ),
            ("interner", digest(|h| self.interner.fold_names(h))),
        ]
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn hash_owner_diagnostics_for_test(&self) -> [(&'static str, u64); 17] {
        // Preserve the existing snapshot tests' owner selection, houses
        // through projectiles; the new production prefix additionally
        // includes clocks and Scenario RNG.
        self.state_hash_owners()[1..18]
            .try_into()
            .expect("17 retained owner groups")
    }

    /// Deterministic state hash over canonicalized simulation state.
    ///
    /// Hashes clocks, Scenario RNG, production, fog, alliances, and all entity
    /// components in stable-entity-ID order (EntityStore keys_sorted) for determinism.
    pub fn state_hash(&self) -> u64 {
        let mut hasher = StateHasher::new();
        self.fold_state(&mut hasher);
        hasher.finish()
    }

    /// Fold everything `state_hash` covers into `hasher`, in a fixed order.
    ///
    /// The per-House fog digests, the per-entity digests and the scans for a
    /// cell or entity that retains the shared dummy cell do not depend on the
    /// fold before them, so they run on the worker pool while this thread
    /// folds the parts that come first. The fold order, and so the hash, does
    /// not change.
    fn fold_state(&self, hasher: &mut impl Hasher) {
        let mut fog_digests = Vec::new();
        parallel::alongside(
            || fog_digests = self.fog_digests(),
            || self.fold_before_fog(hasher),
        );
        let entities: Vec<_> = self.substrate.entities.values().collect();
        let mut entity_digests = Vec::new();
        let mut cells_or_entities_retain_dummy = false;
        parallel::alongside(
            || {
                entity_digests = self.entity_digests(&entities);
                cells_or_entities_retain_dummy =
                    self.cells_anchor_dummy() || self.entities_target_dummy(&entities);
            },
            || self.fold_fog_through_projectiles(&fog_digests, hasher),
        );
        self.hash_shared_cell_dummy(cells_or_entities_retain_dummy, hasher);
        self.hash_waves(hasher);
        self.disk_lasers.hash_state(hasher);
        self.hash_super_weapons(hasher);
        Self::hash_entities(&entity_digests, hasher);
        self.hash_anims(hasher);
        self.hash_voxel_anims(hasher);
        self.substrate.spotlights.hash_state(hasher);
        self.hash_particle_systems(hasher);
        self.session.fold_identity(self.houses.order(), hasher);
        self.session.pixel_conversion_bounds().hash(hasher);
    }

    /// Offline, ScenarioClass owns the sole synchronized RNG; Main and MapGen
    /// are process globals. Network MapGen is agreed at fresh start
    /// ([`Simulation::install_session_mode`]) because bridge repair consumes
    /// it to select hashed overlay IDs. This reaches the state hash the start
    /// agreement compares, not the per-frame sync checksum.
    fn hash_network_mapgen(&self, hasher: &mut impl Hasher) {
        if self.session.execution_topology().is_lockstep() {
            b"network-mapgen-v1".hash(hasher);
            self.mapgen_rng.hash_state(hasher);
        }
    }

    /// The fold before the fog digests: clocks, RNG, identities, scripts,
    /// object order, occupation, Houses, production and power.
    fn fold_before_fog(&self, hasher: &mut impl Hasher) {
        self.session.tick().hash(hasher);
        self.session.binary_frame().hash(hasher);
        self.planning.hash_state(hasher);
        self.hash_ion_blasts(hasher);
        #[cfg(feature = "phobos")]
        self.warhead_anim_runtime.hash_state(hasher);
        // ScenarioClass's RNG; Main is a process global.
        self.scenario_rng.hash_state(hasher);
        self.hash_network_mapgen(hasher);
        // Scenario+214 controls identities of future constructors and therefore
        // guided Bullet steering. The saved prefix/phase also controls a fresh
        // map-read continuation. This is independent of Rust stable handles.
        self.native_unique_ids.hash(hasher);
        self.hash_type_tech_levels(hasher);
        if let Some(made_up) = self.ares_made_up_rules.as_ref() {
            b"ares-made-up-rules-v1".hash(hasher);
            made_up.hash(hasher);
        }
        let mut voxel_damage = self.voxel_type_damage_identity_entries().peekable();
        if voxel_damage.peek().is_some() {
            b"voxel-type-damage-v1".hash(hasher);
            for (id, name, damage) in voxel_damage {
                id.0.hash(hasher);
                name.hash(hasher);
                damage.hash(hasher);
            }
        }
        if !self.building_upgrade_art_names().is_empty() {
            b"building-upgrade-type-art-v458".hash(hasher);
            self.building_upgrade_art_names().hash(hasher);
        }
        self.substrate.next_stable_object_id.hash(hasher);
        self.substrate.next_air_tracker_order.hash(hasher);
        // YR LogicClass trigger latches are save/lockstep state, even though
        // their camera/message outcomes stay app-owned and are not hashed.
        self.trigger_runtime.hash_state(hasher);
        // Wasm extension modules; absent from stock play.
        if !self.ext_modules.is_empty() {
            self.ext_modules.hash_state(hasher);
        }
        // ObjectClass+0x64, written by trigger action 99 and saved with the object.
        if !self.object_custom_sounds.is_empty() {
            b"object-custom-sounds-v1".hash(hasher);
            self.object_custom_sounds.hash(hasher);
        }
        self.team_script_vm
            .hash_state(self.session.binary_frame() as i32, hasher);
        self.hash_playfield_authority(hasher);

        // LogicClass active-object order — authoritative (drives reconciliation order).
        let order = self.substrate.logic.as_slice();
        order.len().hash(hasher);
        for id in order {
            id.hash(hasher);
        }
        self.substrate.display.fold_hash(hasher);

        // PendingDeleteList is an independent ordered substrate fact. The
        // length delimiter distinguishes queue boundaries before the ordered
        // IDs are folded (duplicates are intentionally preserved here).
        self.substrate.pending_delete.hash_state(hasher);
        if !self.substrate.ares_temporal_factory_refresh().is_empty() {
            b"ares-temporal-factory-refresh-v1".hash(hasher);
            self.substrate.ares_temporal_factory_refresh().hash(hasher);
        }
        self.load_objects.hash_state(hasher);
        // Path searches waiting for the next frame's batch.
        if let Some(batch) = &*self.path_batch {
            b"path-batch-v2".hash(hasher);
            batch.hash(hasher);
        }
        // Whether target scans are batched, which also spreads the patrol
        // scans and updates teams on their own turns; the scans' answers live
        // within a pass.
        if self.scan_batch.is_some() {
            b"scan-batch-v1".hash(hasher);
        }
        // Whether turns draw from their own streams (`turn_streams`).
        if *self.turn_streams {
            b"turn-streams-v1".hash(hasher);
        }

        self.substrate.fold_raw_cell_occupation(hasher);
        self.substrate.fold_hidden_occupation(hasher);
        self.substrate.fold_air_slots(hasher);
        self.substrate.fold_base_reservations(hasher);
        self.substrate.occupancy.hash_memberships(hasher);

        self.session.fold_game_options(hasher);
        self.hash_houses(hasher);
        self.hash_terminal_score_snapshot(hasher);
        self.hash_production(hasher);
        self.hash_power_states(hasher);
    }

    /// The fold from the fog digests through projectiles.
    fn fold_fog_through_projectiles(&self, fog_digests: &[u64], hasher: &mut impl Hasher) {
        self.hash_fog_and_alliances(fog_digests, hasher);
        self.hash_bridge_state(hasher);
        self.pending_bridge_collapses.hash_state(hasher);
        // Native local graph updates retain route-selection history even
        // when the current terrain and House threat values are identical.
        // Ordinary native load rebuilds the hierarchy through581F50; its
        // resulting hash may intentionally differ from the live pre-save hash.
        if let Some(zones) = &*self.zone_grid {
            b"retained-navigation-history-v1".hash(hasher);
            zones.fold_navigation_history(hasher);
        }
        // `resolved_terrain` is derived/skipped. Fold the exact saved real
        // CellClass `0x1180` values once through their serialized authority.
        b"real-cell-bridge-flags-v2".hash(hasher);
        self.real_cell_bridge_flags_0x1180.hash(hasher);
        b"dynamic-terrain-cells-v2".hash(hasher);
        self.dynamic_terrain_cells.hash_simulation(hasher);
        self.hash_ice_cracks(hasher);
        self.hash_terrain_dent(hasher);
        if let Some(terrain) = &*self.resolved_terrain {
            terrain.hash_burrowing_permissions(hasher);
        }
        self.hash_overlay_grid(hasher);
        self.hash_crate_authority(hasher);
        self.hash_smudge_grid(hasher);
        self.hash_radiation(hasher);
        self.kamikaze.fold_hash(hasher);
        self.hash_projectiles(hasher);
    }

    /// Whether something retains the shared dummy cell, so that its live
    /// coordinate is state ([`Self::hash_shared_cell_dummy`] folds it then).
    /// Otherwise nothing reads the coordinate: the next lookup that hands
    /// the dummy out stamps it first.
    pub(super) fn shared_cell_dummy_retained(&self) -> bool {
        let entities: Vec<_> = self.substrate.entities.values().collect();
        self.cells_anchor_dummy()
            || self.entities_target_dummy(&entities)
            || self.dummy_retained_beyond_cells_and_entities(&self.effective_shared_cell_dummy())
    }

    /// Whether the dummy's own bridge anchor, a Team script or a projectile
    /// retains it.
    fn dummy_retained_beyond_cells_and_entities(
        &self,
        dummy: &crate::map::resolved_terrain::SharedCellDummy,
    ) -> bool {
        dummy.native_anchor() == Some(crate::map::cell_index::NativeCellIdentity::Dummy)
            || self
                .resolved_terrain
                .as_ref()
                .is_some_and(|terrain| self.team_script_vm.retains_dummy_cell(terrain))
            || self.projectiles.iter().any(|(_, projectile)| {
                projectile.target() == crate::sim::projectile::ProjectileTarget::DummyCell
            })
    }

    /// The shared dummy cell's state, and its live coordinate while its own
    /// bridge anchor, a real cell's bridge anchor or an entity's target
    /// (`cells_or_entities_retain_dummy`), or a projectile retains it.
    fn hash_shared_cell_dummy(
        &self,
        cells_or_entities_retain_dummy: bool,
        hasher: &mut impl Hasher,
    ) {
        let shared_dummy_handle = self.effective_shared_cell_dummy();
        let shared_dummy = shared_dummy_handle.snapshot();
        let gap_flags = shared_dummy_handle.retained_bridge_flags() & 0xC00;
        {
            let extra_flags = shared_dummy_handle.raw_flags()
                & !crate::map::bridge_facts::RETAINED_CELLCLASS_BRIDGE_FLAG_MASK;
            let anchor = shared_dummy_handle.native_anchor();
            if extra_flags != 0 || anchor.is_some() {
                b"shared-cell-dummy-bridge-publication-v1".hash(hasher);
                extra_flags.hash(hasher);
                anchor.hash(hasher);
            }
        }
        if gap_flags != 0 {
            b"shared-cell-dummy-gap-v1".hash(hasher);
            gap_flags.hash(hasher);
        }
        let shared_dummy_overlay = shared_dummy_handle.overlay_identity_state();
        b"shared-cell-dummy-land-v1".hash(hasher);
        shared_dummy_handle.land_type().hash(hasher);
        if shared_dummy_handle.neighbor_count() != 0 {
            b"shared-cell-dummy-neighbor-count-v1".hash(hasher);
            shared_dummy_handle.neighbor_count().hash(hasher);
        }
        let shared_dummy_tube = shared_dummy_handle.raw_tube_index();
        if shared_dummy_tube != -1 {
            b"shared-cell-dummy-tube-v1".hash(hasher);
            shared_dummy_tube.hash(hasher);
        }
        // Unlike the requested coordinate, native `+0x140 & 0x1180`
        // survives ordinary lookups and changes later bridge/FNPC/target
        // behavior even when no Bullet currently retains the dummy.
        b"shared-cell-dummy-spark-v4".hash(hasher);
        shared_dummy.bridge_flags_0x1180.hash(hasher);
        shared_dummy.level.hash(hasher);
        shared_dummy.slope_type.hash(hasher);
        // CellClass+0x44/+0x11E can be mutated through true-dummy wall
        // cleanup and later changes lookup-dependent wall behavior. Native
        // Resize reconstructs the process object, so this is synchronized
        // live-state authority rather than Scenario payload authority.
        b"shared-cell-dummy-overlay-v1".hash(hasher);
        shared_dummy_overlay.hash(hasher);
        if cells_or_entities_retain_dummy
            || self.dummy_retained_beyond_cells_and_entities(&shared_dummy_handle)
        {
            // Retained Cell pointers make the live coordinate affect future
            // range, Team orders, approach and projectile behavior.
            b"shared-cell-dummy-target-v3".hash(hasher);
            shared_dummy.coord.hash(hasher);
        }
    }

    /// Whether an allocated real cell's bridge anchor is the shared dummy
    /// cell.
    fn cells_anchor_dummy(&self) -> bool {
        self.resolved_terrain.as_ref().is_some_and(|terrain| {
            let cells = terrain.cells();
            parallel::any_index(cells.len(), |index| {
                terrain.is_allocated(index)
                    && cells[index].bridge_facts.native_anchor
                        == Some(crate::map::cell_index::NativeCellIdentity::Dummy)
            })
        })
    }

    /// Whether an entity's attack, suspended, archive or navigation target is
    /// a cell the shared dummy stands for.
    fn entities_target_dummy(&self, entities: &[&GameEntity]) -> bool {
        self.resolved_terrain.as_ref().is_some_and(|terrain| {
            use crate::map::cell_index::NativeCellIdentity;
            use crate::sim::combat::TargetKind;
            use crate::sim::components::NavTargetRef;
            let keeps_dummy = |target: TargetKind| {
                target.cell_identity(terrain) == Some(NativeCellIdentity::Dummy)
            };
            parallel::any_index(entities.len(), |index| {
                let entity = entities[index];
                if entity.seldom.patrol_post.cell() == Some(NativeCellIdentity::Dummy) {
                    return true;
                }
                [
                    entity.attack_target.target(),
                    entity.seldom.suspended_attack_target.target(),
                    entity.archive_target(),
                ]
                .into_iter()
                .flatten()
                .any(keeps_dummy)
                    || [
                        entity.navigation.nav_com(),
                        entity.navigation.nav_com_aux(),
                        entity.navigation.suspended_nav_com(),
                    ]
                    .into_iter()
                    .flatten()
                    .chain(entity.navigation.nav_queue().iter().copied())
                    .chain(entity.navigation.nav_list().iter().copied())
                    .any(|target| match target {
                        NavTargetRef::Cell { rx, ry } => keeps_dummy(TargetKind::Cell(rx, ry)),
                        _ => false,
                    })
            })
        })
    }

    /// Fold mutable MapClass LocalSize authority and its immutable Size-height
    /// normalization input. Trigger action 0x28 changes these bounds inside the
    /// synchronized master frame (`TriggerAction__Execute @ 0x006DD8B0`), so a
    /// lockstep hash that omitted them could accept divergent later placement,
    /// path-zone, and trigger behavior.
    fn hash_playfield_authority(&self, hasher: &mut impl Hasher) {
        if self.playfield_bounds.is_none()
            && self.playfield_size_height.is_none()
            && *self.playfield_revision == 0
        {
            // Preserve the historical headless-fixture stream while still
            // distinguishing every configured authority from absence.
            return;
        }
        b"playfield-authority-v2".hash(hasher);
        match *self.playfield_bounds {
            None => 0u8.hash(hasher),
            Some(bounds) => {
                1u8.hash(hasher);
                bounds.base.hash(hasher);
                bounds.off_fc.hash(hasher);
                bounds.off_100.hash(hasher);
                bounds.off_104.hash(hasher);
                bounds.off_108.hash(hasher);
            }
        }
        self.playfield_size_height.hash(hasher);
        self.playfield_revision.hash(hasher);
    }

    fn hash_terminal_score_snapshot(&self, hasher: &mut impl Hasher) {
        let Some(snapshot) = self.terminal_score_snapshot.snapshot() else {
            return;
        };
        b"terminal-score-v1".hash(hasher);
        snapshot.rows.len().hash(hasher);
        for row in &snapshot.rows {
            row.owner.hash(hasher);
            row.country.hash(hasher);
            row.survived.hash(hasher);
            row.kills.hash(hasher);
            row.losses.hash(hasher);
            row.built.hash(hasher);
            row.raw_score.hash(hasher);
            row.score.hash(hasher);
        }
    }

    fn hash_projectiles(&self, hasher: &mut impl Hasher) {
        self.projectiles.hash_scalable_runtime(hasher);
        self.projectiles.len().hash(hasher);
        for (&id, projectile) in self.projectiles.iter() {
            id.hash(hasher);
            projectile.id().hash(hasher);
            projectile.native_unique_id().hash(hasher);
            projectile.source_id().hash(hasher);
            projectile.position().x.hash(hasher);
            projectile.position().y.hash(hasher);
            projectile.position().z.hash(hasher);
            projectile.launch_origin().hash(hasher);
            projectile.launch_target().hash(hasher);
            projectile.previous_cell().hash(hasher);
            hash_projectile_target(projectile.target(), hasher);
            projectile.last_target_position().x.hash(hasher);
            projectile.last_target_position().y.hash(hasher);
            projectile.last_target_position().z.hash(hasher);
            projectile.payload().base_damage.hash(hasher);
            projectile.payload().warhead.index().hash(hasher);
            projectile
                .payload()
                .projectile_type()
                .map(|id| id.index())
                .hash(hasher);
            projectile.payload().bright().hash(hasher);
            projectile
                .payload()
                .installed_weapon()
                .map(|id| id.index())
                .hash(hasher);
            if projectile.payload().damage_multiplier()
                != crate::sim::projectile::ProjectilePayload::UNSCALED
            {
                b"prism-damage-multiplier-v1".hash(hasher);
                projectile.payload().damage_multiplier().hash(hasher);
            }

            projectile.speed_leptons_per_frame().hash(hasher);
            projectile.velocity().hash(hasher);
            projectile.trajectory().hash(hasher);
            projectile.guidance().hash(hasher);
            projectile.visual().hash(hasher);
            projectile.arm_timer().hash(hasher);
            projectile.fuse_frames_remaining().hash(hasher);
            projectile.ranged_fuse().hash(hasher);
            projectile.proximity_reference().hash(hasher);
            projectile.last_distance_half().hash(hasher);
            projectile.tracks_target().hash(hasher);
            projectile.target_expiry().hash(hasher);
            projectile.collision().level.hash(hasher);
            projectile.collision().subject_to_walls.hash(hasher);
            projectile.collision().native_cell_collision.hash(hasher);
            projectile.collision().dropping.hash(hasher);
            projectile.collision().subject_to_cliffs.hash(hasher);
            projectile.collision().flak_scatter.hash(hasher);
            projectile.collision().anti_air.hash(hasher);
            projectile.collision().airburst.hash(hasher);
            // Phobos `Splits=` can arm the airburst tail without `Airburst=`;
            // stock bullets never differ, so their hash is unchanged.
            if projectile.collision().explodes_airburst() != projectile.collision().airburst {
                b"bullet-explodes-airburst-v1".hash(hasher);
            }
            projectile.collision().inaccurate.hash(hasher);
            projectile.collision().floater.hash(hasher);
            projectile.collision().elasticity_bits.hash(hasher);
            projectile.on_bridge().hash(hasher);
            projectile.collision().arcing.hash(hasher);
            if projectile.collision().subject_to_solid {
                b"ares-bullet-subject-to-solid-v1".hash(hasher);
            }
            if let Some(house) = projectile.firer_house() {
                b"bullet-firer-scheme-v1".hash(hasher);
                house.index().hash(hasher);
            }
            #[cfg(feature = "phobos")]
            if let Some(house) = projectile.phobos_firer_house() {
                b"phobos-bullet-firer-house-v1".hash(hasher);
                house.index().hash(hasher);
            }
            if let Some(sw_type) = projectile.ares_nuke_sw() {
                b"ares-bullet-nuke-sw-v1".hash(hasher);
                self.interner.resolve(sw_type).hash(hasher);
            }
            #[cfg(feature = "phobos")]
            if projectile.payload().firepower_multiplier()
                != crate::util::native_x87::NativeF64Bits::ONE
            {
                b"phobos-bullet-firepower-mult-v1".hash(hasher);
                projectile
                    .payload()
                    .firepower_multiplier()
                    .bits()
                    .hash(hasher);
            }
            if projectile.awaiting_anim() {
                b"bullet-awaited-anim-v1".hash(hasher);
                projectile.awaited_anim().hash(hasher);
            }
        }
    }

    fn hash_waves(&self, hasher: &mut impl Hasher) {
        self.active_wave_links.len().hash(hasher);
        for (&owner_id, &wave_id) in &*self.active_wave_links {
            owner_id.hash(hasher);
            wave_id.hash(hasher);
        }
        self.waves.len().hash(hasher);
        for (&id, wave) in self.waves.iter() {
            id.hash(hasher);
            wave.hash(hasher);
        }
    }

    /// Hash all particle systems in stable-id order (BTreeMap iteration).
    /// Each system contributes its type, position, lifetime, and ordered particle list.
    fn hash_particle_systems(&self, hasher: &mut impl Hasher) {
        self.particle_systems().len().hash(hasher);
        if let Some(id) = self.particle_systems().gas_cloud_system() {
            b"global-gas-cloud-system-v405".hash(hasher);
            id.hash(hasher);
        }
        for (id, sys) in self.particle_systems().iter() {
            id.hash(hasher);
            sys.native_unique_id().hash(hasher);
            sys.type_id().0.hash(hasher);
            sys.coords().x.hash(hasher);
            sys.coords().y.hash(hasher);
            sys.coords().z.hash(hasher);
            sys.lifetime().hash(hasher);
            sys.facing().hash(hasher);
            sys.done_spawning().hash(hasher);
            if sys.owner_fire_link() {
                0x304_u16.hash(hasher);
            }
            sys.alive().hash(hasher);
            sys.in_limbo().hash(hasher);
            // The rest of the saved system state. `in_logic_vector` is not
            // saved (load rebuilds it from the logic vector), so it stays out.
            sys.offset().to_array().hash(hasher);
            sys.spawn_timer().bits().hash(hasher);
            sys.spark_spawn_frames().hash(hasher);
            sys.target_ref().hash(hasher);
            sys.owner_entity().hash(hasher);
            sys.target_coords().to_array().hash(hasher);
            sys.owner_house().hash(hasher);
            sys.particles().len().hash(hasher);
            for particle in sys.particles() {
                hash_particle(particle, hasher);
            }
            if let Some(ext) = sys.ares() {
                ext.hash_state(hasher);
            }
        }
        let retiring = self.particle_systems().detached_particles();
        if retiring.len() != 0 {
            b"detached-particles-v397".hash(hasher);
            retiring.len().hash(hasher);
            for (id, particle, alive, in_limbo) in retiring {
                id.hash(hasher);
                alive.hash(hasher);
                in_limbo.hash(hasher);
                hash_particle(particle, hasher);
            }
        }
    }

    /// The owner's stored buildings and other objects whose destruction is
    /// not yet recorded (test observation of the retired per-house counts).
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn owned_object_counts(&self, owner: crate::sim::intern::InternedId) -> (u32, u32) {
        self.substrate
            .entities
            .values()
            .filter(|entity| entity.owner() == owner && !entity.seldom.kill_record.is_recorded())
            .fold((0, 0), |(buildings, others), entity| {
                if entity.category() == crate::map::entities::EntityCategory::Structure {
                    (buildings + 1, others)
                } else {
                    (buildings, others + 1)
                }
            })
    }

    /// Hash per-player house state (BTreeMap = deterministic order).
    fn hash_houses(&self, hasher: &mut impl Hasher) {
        for (owner, house) in &*self.houses {
            owner.hash(hasher);
            if !house.permanent_factory_plans().is_empty() {
                b"ares-permanent-factory-plans-v1".hash(hasher);
                house.permanent_factory_plans().hash(hasher);
            }
            //House50D610/50D320 history survives object death and affects
            //future restoration. The House owns its ordered private records.
            house.hash_initial_records(hasher);
            // Retained509400/481870 values affect future path/AI decisions.
            // The all-zero constructor state preserves historical hashes.
            let threat = house.spatial_threat_values();
            if threat.iter().any(|&value| value != 0) {
                b"house-spatial-threat-v1".hash(hasher);
                threat.hash(hasher);
            }
            house.economy().credits().hash(hasher);
            // The sole cash balance retains its original position in the hash stream.
            house.economy().spent_credits().hash(hasher);
            // The score feeds the terminal Scenario draw.
            house.economy().score().hash(hasher);
            // Ares' tiberium stores, once any holds tiberium.
            house.economy().ares_storage().hash_retained(hasher);
            // House504080/503040 preserves the kill, loss and built totals
            // through native load. The retained-ship chain needs both its
            // initial and terminal loss. Preserve zero streams.
            if *house.stats() != crate::sim::house_state::MatchStatistics::default() {
                b"house-match-statistics-v1".hash(hasher);
                house.stats().hash(hasher);
            }
            house.side_index().hash(hasher);
            house.is_human().hash(hasher);
            if self.session.execution_topology().is_lockstep() {
                // Lifecycle discovery projects every represented human before
                // shared Event5 reads this notification; it is no longer a
                // process-local cache in a network world.
                house.discovered_by_current_house().hash(hasher);
            }
            // CreateHouses688109..68810F sets PlayerControl(+1ED) only on
            // this process's source. LAN/WOL peers intentionally differ in
            // that local input/EVA selector; IsHuman remains shared authority.
            // Campaign admission/initial credits still read PlayerControl.
            // Source: original gamemd.exe instructions.
            if !self.session.execution_topology().is_lockstep() {
                house.player_control().hash(hasher);
            }
            (house.difficulty() as i32).hash(hasher);
            house.rof_bias().bits().hash(hasher);
            house.is_defeated().hash(hasher);
            if house.is_to_die() {
                b"house-flag-to-die-v1".hash(hasher);
            }
            if house.civilians_evacuated() {
                b"house-civilians-evacuated-v1".hash(hasher);
            }
            house.has_won().hash(hasher);
            house.has_lost().hash(hasher);
            // Compatibility fold of the pending view derived from these
            // flags; no separate result authority or session exit latch.
            house.outcome().hash(hasher);
            house.map_is_clear().hash(hasher);
            house.spy_sat_active().hash(hasher);
            // The initial sight latch drives future per-House fog callbacks.
            // Network sight updates every represented human viewpoint, so this
            // retained invalidation state is shared and must remain checked.
            if house.initial_sight_refresh_pending() {
                b"house-initial-sight-refresh-v1".hash(hasher);
            }
            house.tracking().hash_defeat_counters(hasher);
            if house.tech_recheck_pending() {
                b"house-tech-recheck-v1".hash(hasher);
            }
            // House+0x164/+0x168 (Tech Hospital / Machine Shop). Tagged so a
            // house that never owned one keeps the historical stream, as the
            // spatial-threat and match-statistics folds above do.
            if house.self_heal_infantry() != 0 || house.self_heal_units() != 0 {
                b"house-self-heal-v1".hash(hasher);
                house.self_heal_infantry().hash(hasher);
                house.self_heal_units().hash(hasher);
            }
            house.tech_level().hash(hasher);
            hash_house_ai_activation_fields(house, hasher);
            house.strategy_emergency().hash(hasher);
            house.grudge_scores().len().hash(hasher);
            for (other, score) in house.grudge_scores() {
                other.hash(hasher);
                score.hash(hasher);
            }
            house.enemy_house().hash(hasher);
            house.hash_scouted_houses(hasher);

            if let Some((rx, ry)) = house.base_center() {
                1u8.hash(hasher);
                rx.hash(hasher);
                ry.hash(hasher);
            } else {
                0u8.hash(hasher);
            }
            if house.base_radius() != 0 {
                b"house-base-radius-v1".hash(hasher);
                house.base_radius().hash(hasher);
            }
            house.alternate_base_center().hash(hasher);
            if !house.build_const_order().is_empty() {
                b"naval-build-const-house-v1".hash(hasher);
                house.build_const_order().len().hash(hasher);
                for stable_id in house.build_const_order() {
                    stable_id.hash(hasher);
                }
            }
            if !house.base_projection().buildings().is_empty() {
                b"house-buildings-v1".hash(hasher);
                house.base_projection().buildings().hash(hasher);
            }
            if !house.base_projection().factory_plants().is_empty() {
                b"house-factory-plants-v1".hash(hasher);
                house.base_projection().factory_plants().hash(hasher);
            }
            house.base_plan().percent_built().hash(hasher);
            house.base_plan().nodes().len().hash(hasher);
            for node in house.base_plan().nodes() {
                node.type_or_control.hash(hasher);
                node.packed_cell.hash(hasher);
                node.filled.hash(hasher);
                node.retry_count.hash(hasher);
            }
            house.base_plan_center().hash(hasher);
            house.base_reservation().hash(hasher);
            house.waypoint_edge().hash(hasher);
            // `HouseClass+0x242`, the sticky harvester no-ore latch — a
            // raw House byte in the native save block and lockstep CRC.
            house.harvester_no_ore().hash(hasher);
            // `HouseClass+0x57D4` funds-nag TimerStruct and the
            // `[0xA8F040]` low-power guard (`HouseClass::Update
            // 0x004F8B3C..0x004F8DAB`): both live in the native save.
            // Folded with a 64-bit start, as before the timer was a
            // `CdTimer`.
            i64::from(house.eva_funds_timer().start_frame()).hash(hasher);
            house.eva_funds_timer().duration().hash(hasher);
            house.eva_low_power_guard().hash(hasher);
            house.hash_event_notifications(hasher);
            house.hash_production_history(hasher);
            house.hash_lost_building(hasher);
            house.hash_spy_gains(hasher);
            house.hash_powered_unit_centers(hasher);
            house.hash_ares_last_super(hasher);
            house.hash_native_construction(hasher);
            house.hash_difficulty_biases(hasher);
            house.hash_scenario_parameters(hasher);
            house.hash_allow_win_tags(hasher);
            house.hash_super_weapon_targets(hasher);
            house.repair_delay().to_bits().hash(hasher);
            house.repair_start_latch().hash(hasher);
            i64::from(house.repair_latch_timer().start_frame()).hash(hasher);
            house.repair_latch_timer().duration().hash(hasher);
            // Tagged and folded only off their constructor values, so a house
            // without computer production hashes as earlier schemas did.
            let gatherers = house.tracking().resource_gatherers();
            if *house.ai_production() != Default::default() || gatherers != 0 {
                b"ai-base-building-v1".hash(hasher);
                house.ai_production().hash(hasher);
                gatherers.hash(hasher);
            }
            let forces = house.tracking().force_values();
            if forces != Default::default() {
                b"house-force-values-v1".hash(hasher);
                forces.hash(hasher);
            }
            let docks = house.tracking().airport_docks();
            if docks != 0 {
                b"house-airport-docks-v1".hash(hasher);
                docks.hash(hasher);
            }
            let factories = house.tracking().factories();
            if factories != [0; 5] {
                b"house-factory-counters-v1".hash(hasher);
                factories.hash(hasher);
            }
            let war_factories = house.tracking().war_factories();
            if war_factories != 0 {
                b"house-war-factories-v1".hash(hasher);
                war_factories.hash(hasher);
            }
            if *house.strategy_timer() != crate::sim::house_state::strategy_timer_at_construction()
            {
                b"house-strategy-timer-v1".hash(hasher);
                house.strategy_timer().hash(hasher);
            }
            if *house.enemy_search_timer()
                != crate::sim::house_state::enemy_search_timer_at_construction()
            {
                b"house-enemy-search-timer-v1".hash(hasher);
                house.enemy_search_timer().hash(hasher);
            }
            house.team_creation().hash_state(hasher);
            if *house.ai_unit_choices() != Default::default() {
                b"house-ai-unit-choices-v1".hash(hasher);
                house.ai_unit_choices().hash(hasher);
            }
            house.tracking().hash_ai_team_counters(hasher);
        }
    }

    /// Hash production (primary flags, the enqueue stamp, the factory
    /// registry), then ore growth and the terrain objects; the fold order is
    /// part of the hash contract.
    fn hash_production(&self, hasher: &mut impl Hasher) {
        let retired_tiberium_fold = false;
        // The queue-of-record lives in the factory registry (active build = `Factory`
        // head fields; tail = `Factory.queue` of `QueueEntry`), and so does a completed
        // building waiting for placement; both fold in `hash_factory_registry`.
        // `remaining_base_frames` no longer exists (it was a `BuildQueueItem` field);
        // the sidebar ETA derives it from `progress` at view time and it is
        // intentionally NOT hashed (the 18->19 shape change).
        self.production.hash_primary_factories(hasher);
        self.production.next_enqueue_order().hash(hasher);
        self.hash_factory_registry(hasher); // P5b: the authoritative factory registry

        // Live ore/gem identity and quantity are folded by `hash_overlay_grid`.
        self.ore_growth_config.hash_enabled(hasher);
        self.ore_growth_state
            .hash_state(hasher, retired_tiberium_fold);
        // Retained terrain animations (TIBTRE ore generators among them),
        // the objects and their cell and spawner indexes.
        self.terrain_objects.hash_state(hasher);
        if retired_tiberium_fold {
            // The fallback ore overlay id: `None` unless terrain spawners were
            // seeded from a registry, which the pinned fixtures never did.
            None::<u8>.hash(hasher);
        }
    }

    /// Hash the authoritative factory registry in the deterministic temporal sweep
    /// order (`iter_insertion_ordered`, by `insertion_seq` = front `enqueue_order`) —
    /// the SAME order `step_all` charges in, so the fold order is part of the hash
    /// contract. Explicit-field folding (NOT `#[derive(Hash)]`) so the Option
    /// presence tags fold distinctly, consistent with the rest of this file.
    fn hash_factory_registry(&self, hasher: &mut impl Hasher) {
        for (holder, f) in self.production.factories().holders_insertion_ordered() {
            // The building that holds a computer's factory (`BuildingClass+0x524`);
            // a House's factory folds as earlier schemas did.
            if let crate::sim::production::FactoryHolder::Building(building) = holder {
                b"building-factory-v1".hash(hasher);
                building.hash(hasher);
            }
            f.hash_state(hasher);
        }
    }

    /// Hash per-player power states for deterministic replay.
    fn hash_power_states(&self, hasher: &mut impl Hasher) {
        // BTreeMap<InternedId, _> iterates in deterministic sorted order.
        for (owner_id, state) in self.power_states.iter() {
            owner_id.hash(hasher);
            state.total_output().hash(hasher);
            state.total_drain().hash(hasher);
            state.hash_assessment_state(hasher);
            if state.has_drained_power_source() {
                b"House.DrainedPowerSource".hash(hasher);
            }
        }
    }

    /// One digest per House's fog grids ([`FogState::plane_digests`]).
    fn fog_digests(&self) -> Vec<u64> {
        self.fog.plane_digests()
    }

    /// Hash fog-of-war visibility and house alliance data, each House's grids
    /// as its digest from [`Self::fog_digests`].
    fn hash_fog_and_alliances(&self, fog_digests: &[u64], hasher: &mut impl Hasher) {
        self.fog.hash_state(fog_digests, hasher);
        for (owner, allies) in self.house_alliances.as_map() {
            owner.hash(hasher);
            for ally in allies {
                ally.hash(hasher);
            }
        }
    }

    fn hash_bridge_state(&self, hasher: &mut impl Hasher) {
        let Some(bridge_state) = &*self.bridge_state else {
            0u8.hash(hasher);
            return;
        };
        1u8.hash(hasher);
        bridge_state.endpoint_records().len().hash(hasher);
        if let Some(size) = bridge_state.native_zone_source_size() {
            // Geometry affects signed/clamped bridge endpoint projection.
            // The absent synthetic/legacy receipt retains its previous hash.
            0x56c510u32.hash(hasher);
            size.hash(hasher);
        }
        for record in bridge_state.endpoint_records() {
            record.endpoint_a.hash(hasher);
            record.endpoint_b.hash(hasher);
            record.active.hash(hasher);
            record.bridge_kind.hash(hasher);
        }
    }

    fn hash_overlay_grid(&self, hasher: &mut impl Hasher) {
        let Some(overlay_grid) = &*self.overlay_grid else {
            0u8.hash(hasher);
            return;
        };
        1u8.hash(hasher);
        overlay_grid.width().hash(hasher);
        overlay_grid.height().hash(hasher);
        // Row-major order carries the coordinates.
        state_hasher::hash_record_iter(
            overlay_grid.cells().len(),
            overlay_grid.cells().iter(),
            hasher,
            OverlayCell::hash_record,
        );
        b"retained-wall-neighbor-counts-v1".hash(hasher);
        // The presence tag the retired plane-less fixture mode folded as 0
        // keeps its place.
        1u8.hash(hasher);
        overlay_grid.retained_neighbor_counts().hash(hasher);
    }

    /// Fold every raw MapClass crate-slot word in fixed ascending order.
    /// Visible overlays are deliberately not the authority: accepted ghosts
    /// retain future timer behavior without mutating `OverlayGrid`.
    fn hash_crate_authority(&self, hasher: &mut impl Hasher) {
        b"crate-authority-v1".hash(hasher);
        for slot in self.crate_authority().slots() {
            slot.start_frame.hash(hasher);
            slot.aux.hash(hasher);
            slot.duration.hash(hasher);
            slot.cell_x.hash(hasher);
            slot.cell_y.hash(hasher);
        }
    }

    /// Hash persistent cell marks and transient Smudge identities. Constructor
    /// IDs and pending order affect later object identity and expiry consumers.
    fn hash_smudge_grid(&self, hasher: &mut impl Hasher) {
        let Some(grid) = &*self.smudge_grid else {
            0u8.hash(hasher);
            return;
        };
        1u8.hash(hasher);
        let mut entries: Vec<_> = grid
            .iter_occupied()
            .map(|(rx, ry, c)| (rx, ry, c.type_id, c.frame_offset))
            .collect();
        entries.sort();
        entries.len().hash(hasher);
        for e in &entries {
            e.hash(hasher);
        }
        grid.fold_objects(hasher);
    }

    /// Hash the radiation field (cell levels as raw f64 bits — the levels are
    /// products of deterministic numeric operations) and the site registry.
    /// Site order controls future rounded subtractions and belongs in the hash.
    fn hash_radiation(&self, hasher: &mut impl Hasher) {
        for ((rx, ry), level) in self.radiation.iter_cells() {
            rx.hash(hasher);
            ry.hash(hasher);
            level.to_bits().hash(hasher);
        }
        for site in self.radiation.sites_in_update_order() {
            site.hash_into(hasher);
        }
    }

    /// Hash per-house superweapon state and active lightning storm.
    fn hash_super_weapons(&self, hasher: &mut impl Hasher) {
        self.unit_deliveries.hash_state(hasher);
        self.ares_chrono_warps.hash_state(hasher);
        self.ares_firestorm.hash_state(hasher);
        self.ares_tunnels.hash_state(hasher);
        for (owner, weapons) in self.super_weapons.iter() {
            owner.hash(hasher);
            for (type_id, inst) in weapons {
                type_id.hash(hasher);
                inst.is_active().hash(hasher);
                inst.is_ready().hash(hasher);
                inst.is_suspended().hash(hasher);
                inst.charge_start_tick().hash(hasher);
                inst.charge_duration().hash(hasher);
                inst.charge_drain_state().hash(hasher);
                inst.ready_tick().hash(hasher);
                inst.hash_recharge_time(hasher);
                inst.hash_grant_flags(hasher);
                inst.hash_building_charges(hasher);
                inst.hash_paid_selection(hasher);
                inst.hash_ares_launch_history(hasher);
                if let Some(id) = inst.native_unique_id() {
                    b"super-native-identity-v1".hash(hasher);
                    id.hash(hasher);
                }
                // The Chronosphere's source cell and held anim (`+0x62`,
                // `+0x68`); the tag keeps every Super that never took a
                // Chronosphere click on its established stream.
                if inst.chrono_cell() != (0, 0) || inst.placement_anim().is_some() {
                    b"chrono-super-v1".hash(hasher);
                    inst.chrono_cell().hash(hasher);
                    inst.placement_anim().hash(hasher);
                }
                // The Force Shield's countdown and where its sound plays
                // (`+0x50`, `+0x54`); the tag keeps every Super never armed on
                // its established stream.
                let (countdown, coords) = inst.fade();
                if countdown != -1 || coords != [0; 3] {
                    b"force-shield-fade-v1".hash(hasher);
                    countdown.hash(hasher);
                    coords.hash(hasher);
                }
            }
        }
        // The Lightning Storm's globals; a match that never launched one hashes
        // the established `false`.
        let storm_touched = *self.lightning_storm != Default::default();
        storm_touched.hash(hasher);
        if storm_touched {
            self.lightning_storm.hash(hasher);
        }
        // The Psychic Dominator's globals; the tag keeps a match that never
        // launched one on its established stream.
        if *self.psychic_dominator != Default::default() {
            b"psychic-dominator-v1".hash(hasher);
            self.psychic_dominator.hash(hasher);
        }
    }

    /// One digest per entity of `entities`, the stored entities in stable-ID
    /// order (the store iterates in that order, so no sort is needed),
    /// spread over threads in large worlds.
    fn entity_digests(&self, entities: &[&GameEntity]) -> Vec<u64> {
        let frame = self.session.binary_frame();
        let network = self.session.execution_topology().is_lockstep();
        // The visits each sleeper is owed, beside the entities: the callers
        // pass the whole store in its own order. A partial list looks each
        // one up instead.
        let store = &self.substrate.entities;
        let mut owed = store.owed_in_value_order();
        if owed.len() != entities.len() {
            owed = entities
                .iter()
                .map(|entity| store.owed_visits(entity.stable_id()))
                .collect();
        }
        let entities: Vec<(&GameEntity, u32)> = entities.iter().copied().zip(owed).collect();
        digests(
            &entities,
            entities.len() >= 4096,
            |&(entity, owed), hasher| {
                hash_entity_owed(entity, frame, owed, hasher);
                if network {
                    entity.seldom.discovery.hash_network(hasher);
                }
            },
        )
    }

    /// Hash all entity components in stable-entity-ID order, as the digests
    /// from [`Self::entity_digests`].
    fn hash_entities(entity_digests: &[u64], hasher: &mut impl Hasher) {
        for digest in entity_digests {
            digest.hash(hasher);
        }
    }

    /// Scheduler-owned ordinary animations in stable-ID order. Render caches and
    /// transient sound events are deliberately excluded.
    fn hash_anims(&self, hasher: &mut impl Hasher) {
        self.substrate.anims.iter().count().hash(hasher);
        for (id, anim) in self.substrate.anims.iter() {
            id.hash(hasher);

            anim.hash(hasher);
            anim.bounce().is_some().hash(hasher);
            if let Some(body) = &anim.bounce() {
                body.hash_bits(hasher);
            }
        }
    }

    /// `VoxelAnimClass` debris in stable-ID order.
    ///
    /// The physics body is authoritative simulation state, not a render cache:
    /// `VoxelAnimClass::AI @ 0x00749F30` refreshes the object coordinate from it
    /// every tick and reads its `Bounced`/`Stopped` verdict to decide when the
    /// piece dies. The spin axis and angle fold too — they are drawn from the
    /// shared stream by `BounceClass::Init`, so a divergence there is a
    /// divergence in the stream itself, even though nothing in the physics
    /// reads them back.
    fn hash_voxel_anims(&self, hasher: &mut impl Hasher) {
        // Keep the established sparse-store domain: no live debris contributes
        // no bytes. A live piece includes both independent Rust and native
        // identities; the latter also advances the shared Scenario cursor.
        if self.substrate.voxel_anims.is_empty() {
            return;
        }
        b"voxel-anim-debris-v1".hash(hasher);
        self.substrate.voxel_anims.len().hash(hasher);
        for (id, debris) in self.substrate.voxel_anims.iter() {
            id.hash(hasher);
            debris.native_unique_id().hash(hasher);
            debris.type_id().0.hash(hasher);
            debris.world_coord().to_array().hash(hasher);
            debris.lifecycle().object_alive().hash(hasher);
            debris.lifecycle().in_limbo().hash(hasher);
            debris.lifecycle().cell_marked().hash(hasher);
            debris.duration().hash(hasher);
            debris.marked_for_deletion().hash(hasher);
            debris.owner_house().hash(hasher);
            debris.bounce().hash_bits(hasher);
            // CRC74AA63 folds +0x108 only when set; no retail type sets it.
            if let Some(system) = debris.attached_system() {
                system.hash(hasher);
            }
        }
    }
}

/// The same authoritative Particle body survives System removal until725C70.
/// Keep one hash fold for both live children and retiring bodies.
fn hash_particle(p: &crate::sim::particles::Particle, hasher: &mut impl Hasher) {
    p.stable_id().hash(hasher);
    p.native_unique_id().hash(hasher);
    p.type_id().0.hash(hasher);
    p.coords().x.hash(hasher);
    p.coords().y.hash(hasher);
    p.coords().z.hash(hasher);
    p.lifetime_remaining().hash(hasher);
    p.animation_state().hash(hasher);
    p.translucency().hash(hasher);
    p.marked_for_deletion().hash(hasher);
    p.origin().to_array().hash(hasher);
    p.direction().map(|v| v.bits()).hash(hasher);
    p.motion().map(|v| v.bits()).hash(hasher);
    p.velocity().bits().hash(hasher);
    p.damage_counter().hash(hasher);
    p.state_ai_advance().hash(hasher);
    (p.drift_x(), p.drift_y(), p.drift_z()).hash(hasher);
    p.prev_delta().hash(hasher);
    p.target().to_array().hash(hasher);
    p.float_position().map(|value| value.bits()).hash(hasher);
    p.start_rgb().hash(hasher);
    p.color_index().hash(hasher);
    p.color_accumulator().bits().hash(hasher);
}

#[cfg(all(test, tests_sim_world_t_z))]
mod tests;

#[cfg(all(test, tests_sim_world_t_z))]
#[path = "navigation_history_hash_tests.rs"]
mod navigation_history_hash_tests;
