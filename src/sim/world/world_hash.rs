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
use crate::sim::state_hasher::{self, StateHasher};

#[cfg(test)]
mod retained_cell_hash_tests {
    use super::*;
    use crate::map::resolved_terrain::ResolvedTerrainGrid;
    use crate::sim::combat::TargetKind;
    use crate::sim::components::NavTargetRef;
    use crate::sim::game_entity::GameEntity;

    #[test]
    fn retained_cell_targets_hash_live_dummy_coordinate_without_restamping_it() {
        let mut sim = Simulation::new();
        let mut terrain = ResolvedTerrainGrid::from_cells(0, 0, Vec::new());
        terrain.bind_shared_cell_dummy(sim.shared_cell_dummy.clone());
        sim.resolved_terrain = Some(terrain);
        let dummy = sim.effective_shared_cell_dummy();
        let mut entity = GameEntity::test_default(1, "FV", "Test", 10, 10);
        // Every cell is deliberately unallocated. These different requested
        // coordinates retain the same process object; hashing may not stamp it.
        for retained_field in 0..8 {
            entity.attack_target.set_for_test(None);
            entity.suspended_attack_target.set_for_test(None);
            entity.set_archive_target(None);
            entity.navigation = Default::default();
            match retained_field {
                0 => entity
                    .attack_target
                    .set_for_test(Some(TargetKind::Cell(1, 2))),
                1 => entity
                    .suspended_attack_target
                    .set_for_test(Some(TargetKind::Cell(3, 4))),
                2 => entity.set_archive_target(Some(TargetKind::Cell(5, 6))),
                3 => entity
                    .navigation
                    .set_nav_com_for_test(Some(NavTargetRef::cell(7, 8))),
                4 => entity
                    .navigation
                    .set_nav_com_aux_for_test(Some(NavTargetRef::cell(9, 10))),
                5 => entity
                    .navigation
                    .set_suspended_nav_com_for_test(Some(NavTargetRef::cell(11, 12))),
                6 => entity
                    .navigation
                    .push_nav_queue_for_test(NavTargetRef::cell(13, 14)),
                7 => entity
                    .navigation
                    .set_nav_list_for_test(vec![NavTargetRef::cell(15, 16)], false),
                _ => unreachable!(),
            }
            sim.substrate.entities.insert(entity.clone());
            dummy.stamp_coord(99, 98);
            let before = sim.state_hash();
            assert_eq!(dummy.snapshot().coord, (99, 98));
            dummy.stamp_coord(97, 96);
            assert_ne!(before, sim.state_hash(), "retained field {retained_field}");
            assert_eq!(dummy.snapshot().coord, (97, 96));
        }
    }

    #[test]
    fn an_allocated_cell_bridge_anchor_on_the_dummy_hashes_its_live_coordinate() {
        let mut sim = Simulation::new();
        let mut anchored = crate::map::resolved_terrain::test_flat_cell(1, 0);
        anchored.bridge_facts.native_anchor =
            Some(crate::map::cell_index::NativeCellIdentity::Dummy);
        let cells = vec![crate::map::resolved_terrain::test_flat_cell(0, 0), anchored];
        sim.install_resolved_terrain_for_new_map(ResolvedTerrainGrid::from_cells(2, 1, cells));
        let dummy = sim.effective_shared_cell_dummy();
        let coordinate_hashes = |sim: &Simulation| {
            dummy.stamp_coord(5, 6);
            let before = sim.state_hash();
            dummy.stamp_coord(7, 8);
            before != sim.state_hash()
        };
        assert!(coordinate_hashes(&sim));
        let terrain = sim.resolved_terrain.as_mut().expect("installed map");
        terrain.test_set_native_allocated_cells(&[(0, 0)]);
        assert!(
            !coordinate_hashes(&sim),
            "an unallocated cell retains nothing"
        );
        let terrain = sim.resolved_terrain.as_mut().expect("installed map");
        terrain.test_set_native_allocated_cells(&[(0, 0), (1, 0)]);
        let anchor = &mut terrain
            .cell_mut_for_test(1, 0)
            .expect("the cell")
            .bridge_facts;
        anchor.native_anchor = None;
        assert!(
            !coordinate_hashes(&sim),
            "a released anchor retains nothing"
        );
    }
}

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

#[cfg(test)]
mod drive_ship_slope_hash_tests {
    use super::Simulation;
    use crate::map::entities::EntityCategory;
    use crate::rules::locomotor_type::LocomotorKind;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::movement::locomotor::LocomotorState;
    use crate::sim::movement::slope_transition::SlopeTransitionState;

    fn hash_with_state(kind: LocomotorKind, stashed: bool, state: SlopeTransitionState) -> u64 {
        let mut sim = Simulation::new();
        let mut entity = GameEntity::test_default(1, "SLOPE", "Americans", 2, 2);
        entity.set_category_for_test(EntityCategory::Unit);
        let mut locomotor = LocomotorState::for_test_kind(kind);
        *locomotor.active_slope_transition_mut().unwrap() = state;
        if stashed {
            assert!(locomotor.begin_piggyback(LocomotorKind::Teleport, 90,));
        }
        entity.locomotor = Some(locomotor);
        sim.substrate.entities.insert(entity);
        sim.state_hash()
    }

    #[test]
    fn every_active_and_stashed_drive_ship_slope_field_changes_current_hash() {
        let base = SlopeTransitionState::from_fields_for_test(1, 2, 30, 3);
        let variants = [
            SlopeTransitionState::from_fields_for_test(9, 2, 30, 3),
            SlopeTransitionState::from_fields_for_test(1, 9, 30, 3),
            SlopeTransitionState::from_fields_for_test(1, 2, -1, 3),
            SlopeTransitionState::from_fields_for_test(1, 2, 30, 0),
        ];
        for kind in [LocomotorKind::Drive, LocomotorKind::Ship] {
            for stashed in [false, true] {
                let baseline = hash_with_state(kind, stashed, base);
                for variant in variants {
                    assert_ne!(
                        baseline,
                        hash_with_state(kind, stashed, variant),
                        "kind={kind:?} stashed={stashed} must hash every slope field"
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod locomotor_field_hash_tests {
    use super::Simulation;
    use crate::map::entities::EntityCategory;
    use crate::rules::locomotor_type::{LocomotorKind, MovementZone, SpeedType};
    use crate::sim::game_entity::GameEntity;
    use crate::sim::movement::locomotor::{LocomotorState, MovementLayer};

    /// A Teleport unit, optionally on a Drive leg, with `mutate` applied to the
    /// active object or, on the leg, to the suspended Teleport.
    fn hash_with(stashed: bool, mutate: Option<fn(&mut LocomotorState)>) -> u64 {
        let mut sim = Simulation::new();
        let mut entity = GameEntity::test_default(1, "LOCO", "Americans", 2, 2);
        entity.set_category_for_test(EntityCategory::Unit);
        let mut locomotor = LocomotorState::for_test_kind(LocomotorKind::Teleport);
        if stashed {
            assert!(locomotor.begin_piggyback(LocomotorKind::Drive, 0));
        }
        if let Some(mutate) = mutate {
            match locomotor.piggyback_mut() {
                Some(stash) => mutate(stash.suspended_mut_for_test()),
                None => mutate(&mut locomotor),
            }
        }
        entity.locomotor = Some(locomotor);
        sim.substrate.entities.insert(entity);
        sim.state_hash()
    }

    /// Every field outside the class payload (`drive_ship_slope_hash_tests`
    /// and the payload tests cover it) changes the hash, on the active object
    /// and on a suspended one. The six fields #680 found missing from the
    /// active fold are among them.
    #[test]
    fn every_active_and_stashed_locomotor_field_changes_current_hash() {
        type Mutation = (&'static str, fn(&mut LocomotorState));
        let mutations: [Mutation; 6] = [
            ("kind", |l| l.set_kind_for_test(LocomotorKind::Walk)),
            ("powered", |l| l.set_powered_for_test(false)),
            ("layer", |l| l.set_layer_for_test(MovementLayer::Bridge)),
            ("balloon_hover", |l| l.set_balloon_hover_for_test(true)),
            ("speed_type", |l| {
                l.set_speed_type_for_test(SpeedType::Wheel)
            }),
            ("movement_zone", |l| {
                l.set_movement_zone_for_test(MovementZone::Amphibious)
            }),
        ];
        assert_ne!(hash_with(false, None), hash_with(true, None));
        for stashed in [false, true] {
            let baseline = hash_with(stashed, None);
            for (field, mutate) in mutations {
                assert_ne!(
                    baseline,
                    hash_with(stashed, Some(mutate)),
                    "stashed={stashed} must hash {field}"
                );
            }
        }
    }
}

#[cfg(test)]
mod playfield_authority_hash_tests {
    use super::Simulation;
    use crate::map::playfield::PlayfieldBounds;

    fn playfield() -> PlayfieldBounds {
        PlayfieldBounds {
            base: 80,
            off_fc: 2,
            off_100: 4,
            off_104: 76,
            off_108: 48,
        }
    }

    #[test]
    fn state_hash_includes_mutable_playfield_authority() {
        let mut baseline = Simulation::new();
        baseline.playfield_bounds = Some(playfield());
        baseline.playfield_size_height = Some(58);

        let mut changed_bounds = Simulation::new();
        changed_bounds.playfield_bounds = Some(PlayfieldBounds {
            off_100: 5,
            ..playfield()
        });
        changed_bounds.playfield_size_height = Some(58);

        let mut changed_size_height = Simulation::new();
        changed_size_height.playfield_bounds = Some(playfield());
        changed_size_height.playfield_size_height = Some(59);

        let mut changed_revision = Simulation::new();
        changed_revision.playfield_bounds = Some(playfield());
        changed_revision.playfield_size_height = Some(58);
        changed_revision.playfield_revision = 1;

        assert_ne!(baseline.state_hash(), changed_bounds.state_hash());
        assert_ne!(baseline.state_hash(), changed_size_height.state_hash());
        assert_ne!(baseline.state_hash(), changed_revision.state_hash());
    }
}

#[cfg(test)]
mod shared_dummy_bridge_hash_tests {
    use super::Simulation;
    use glam::IVec3;

    use crate::map::bridge_facts::BRIDGE_FLAG_ANCHOR_SELF;
    use crate::map::resolved_terrain::ResolvedTerrainGrid;
    use crate::map::voxel_slope::slope_matrix;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::ruleset::RuleSet;
    use crate::sim::particles::spark::SparkMotionStep;
    use crate::sim::particles::spark_world::SparkCollisionWorld;
    use crate::util::native_x87::NativeF32Bits;

    fn spark_motion_at(x: i32, y: i32, z: i32) -> SparkMotionStep {
        SparkMotionStep {
            old_coords: IVec3::new(x, y, z),
            candidate_coords: IVec3::new(x, y, z),
            candidate_f32: [
                NativeF32Bits::from_bits((x as f32).to_bits()),
                NativeF32Bits::from_bits((y as f32).to_bits()),
                NativeF32Bits::from_bits((z as f32).to_bits()),
            ],
            persistent_velocity: [NativeF32Bits::POSITIVE_ZERO; 3],
            probe_velocity: [NativeF32Bits::POSITIVE_ZERO; 3],
        }
    }

    #[test]
    fn gsi_04_01_hashes_dummy_bridge_bits_without_retained_projectile() {
        let sim = Simulation::new();
        let dummy = sim.effective_shared_cell_dummy();
        let clear_hash = sim.state_hash();

        dummy.set_bridge_flags_0x1180(BRIDGE_FLAG_ANCHOR_SELF);
        let bridge_hash = sim.state_hash();
        assert_ne!(
            clear_hash, bridge_hash,
            "live anchor bit 0x80 alone is future-affecting hash authority"
        );

        dummy.stamp_coord(17, -3);
        assert_eq!(
            bridge_hash,
            sim.state_hash(),
            "without a retained Bullet the requested coordinate stays excluded; 0x1180, level, and slope are unconditional"
        );
    }

    #[test]
    fn bridge_publication_hashes_full_dummy_flags_and_retained_anchor_coordinate() {
        use crate::map::cell_index::NativeCellIdentity;
        let sim = Simulation::new();
        let dummy = sim.effective_shared_cell_dummy();
        let baseline = sim.state_hash();
        dummy.write_raw_flags(0x10000);
        let side_flag = sim.state_hash();
        assert_ne!(baseline, side_flag);
        dummy.write_native_anchor(Some(NativeCellIdentity::Dummy));
        let retained = sim.state_hash();
        assert_ne!(side_flag, retained);
        dummy.stamp_coord(7, -3);
        assert_ne!(
            retained,
            sim.state_hash(),
            "retained bridge pointer makes dummy coordinate future-affecting"
        );
        dummy.write_native_anchor(None);
        assert_eq!(side_flag, sim.state_hash());
    }

    #[test]
    fn gsi_04_03_hashes_dummy_level_slope_without_retained_projectile() {
        let mut sim = Simulation::new();
        sim.install_resolved_terrain_for_new_map(ResolvedTerrainGrid::from_cells(0, 0, Vec::new()));
        let dummy = sim.effective_shared_cell_dummy();
        let terrain_dummy = sim.resolved_terrain.as_ref().unwrap().shared_cell_dummy();
        assert!(
            dummy.same_identity(&terrain_dummy),
            "the hash authority must be the dummy bound into production terrain"
        );
        let rules = RuleSet::from_ini(&IniFile::from_str("")).unwrap();
        let clear_hash = sim.state_hash();

        dummy.set_level_slope(-3, 0);
        let level_facts = SparkCollisionWorld::new(&sim, &rules)
            .unwrap()
            .query(spark_motion_at(320, 192, 300))
            .unwrap();
        assert_eq!(level_facts.ground_z, -311);
        assert!(level_facts.slope_matrix.is_none());
        let level_only_hash = sim.state_hash();
        assert_ne!(
            clear_hash, level_only_hash,
            "dummy level alone is unconditional hash authority"
        );

        dummy.set_level_slope(0, 0);
        assert_eq!(clear_hash, sim.state_hash());

        dummy.set_level_slope(0, 9);
        let slope_facts = SparkCollisionWorld::new(&sim, &rules)
            .unwrap()
            .query(spark_motion_at(320, 192, -100))
            .unwrap();
        assert_eq!(slope_facts.ground_z, 104);
        assert_eq!(slope_facts.slope_matrix, Some(slope_matrix(9).unwrap()));
        let slope_only_hash = sim.state_hash();
        assert_ne!(
            clear_hash, slope_only_hash,
            "dummy slope alone is unconditional hash authority"
        );
    }
}

/// Fold the `MissionCom` mission component into the state hash.
///
/// `MissionCom` intentionally does not derive `Hash`: every lossless selector,
/// raw latch/state dword, and signed timer dword is folded explicitly in the
/// snapshot schema order. Current compatibility projections remain ordinary
/// named writers until the production authority crosswalk is complete.
fn hash_mission_com(mission: &crate::sim::mission::MissionCom, hasher: &mut impl Hasher) {
    mission.current().raw().hash(hasher);
    mission.suspended().raw().hash(hasher);
    mission.queued().raw().hash(hasher);
    mission.movement_bypass_latch().hash(hasher);
    mission.handler_state().hash(hasher);
    mission.mission_start_frame().hash(hasher);
    mission.ai_counter().hash(hasher);
    mission.dispatch_timer().start_frame().hash(hasher);
    mission.dispatch_timer().delay().hash(hasher);
}

fn hash_mission_leaf(leaf: &crate::sim::mission::MissionLeafState, hasher: &mut impl Hasher) {
    if let Some(unit) = leaf.as_unit() {
        0u8.hash(hasher);
        unit.deployed().hash(hasher);
        unit.deploy_begin_active().hash(hasher);
        unit.deploy_reverse_active().hash(hasher);
        unit.tracker_byte_18().hash(hasher);
        unit.tracker_byte_19().hash(hasher);
    } else if let Some(infantry) = leaf.as_infantry() {
        1u8.hash(hasher);
        infantry.firing_sequence_latch().hash(hasher);
        infantry.doing().hash(hasher);
        infantry.pending_deploy().hash(hasher);
        // Infantry ctor517AC2 starts6E8 at2. Water/land DoAction51D8B8
        // retains0/1 before admission, affecting later sound requests.
        // Like the other sparse native bytes, the constructor adds no bytes.
        if infantry.water_state() != 2 {
            b"infantry-water-state-6e8".hash(hasher);
            infantry.water_state().hash(hasher);
        }
    } else if let Some(aircraft) = leaf.as_aircraft() {
        2u8.hash(hasher);
        aircraft.action_latch().hash(hasher);
        aircraft.transition_ready_latch().hash(hasher);
        // Ctor 0x00413D74 starts +0x6D3 at 5, which adds no bytes; Overfly
        // reads it to come round again.
        if aircraft.paradrop_passes() != 5 {
            b"aircraft-paradrop-passes-6d3".hash(hasher);
            aircraft.paradrop_passes().hash(hasher);
        }
    } else if let Some(building) = leaf.as_building() {
        3u8.hash(hasher);
        building.ready_latch().hash(hasher);
        b"building-repair-progress-620".hash(hasher);
        building.repair_progress().hash(hasher);
        // Constructor -1 adds no bytes, preserving earlier streams.
        if building.firing_super_weapon() != -1 {
            b"building-firing-super-weapon-5f8".hash(hasher);
            building.firing_super_weapon().hash(hasher);
        }
    }
    // Infantry already folds the same owned byte above. Native default0
    // retains the prior Unit/Aircraft hash stream; loaded nonzero Foot68D
    // can change a Foot handler and must contribute independently.
    if leaf.as_infantry().is_none() && leaf.foot_firing_sequence_latch() != 0 {
        b"foot-firing-sequence-v1".hash(hasher);
        leaf.foot_firing_sequence_latch().hash(hasher);
    }
    if leaf.foot_idle_entry_latch() != 0 {
        b"foot-idle-entry-6b3".hash(hasher);
        leaf.foot_idle_entry_latch().hash(hasher);
    }
}

impl Simulation {
    /// Read-only Rust diagnostic projections through the existing hash owners.
    /// These are attribution receipts, never gameplay/native golden inputs.
    pub fn state_hash_owners(&self) -> [(&'static str, u64); 15] {
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
                    if let Some(zones) = &self.zone_grid {
                        zones.fold_navigation_history(h);
                    }
                }),
            ),
            (
                "trigger_runtime",
                digest(|h| self.trigger_runtime.hash_state(h)),
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
        ]
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn hash_owner_diagnostics_for_test(&self) -> [(&'static str, u64); 14] {
        // Preserve the existing snapshot tests' owner selection; the new
        // production prefix additionally includes clocks and Scenario RNG.
        self.state_hash_owners()[1..]
            .try_into()
            .expect("14 retained owner groups")
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

    /// The fold before the fog digests: clocks, RNG, identities, scripts,
    /// object order, occupation, Houses, production and power.
    fn fold_before_fog(&self, hasher: &mut impl Hasher) {
        self.session.tick().hash(hasher);
        self.session.binary_frame().hash(hasher);
        self.planning.hash_state(hasher);
        self.hash_ion_blasts(hasher);
        // ScenarioClass owns the sole saved/synchronized RNG. Main and MapGen
        // are process globals and are deliberately absent from this hash.
        self.scenario_rng.hash_state(hasher);
        // Scenario+214 controls identities of future constructors and therefore
        // guided Bullet steering. The saved prefix/phase also controls a fresh
        // map-read continuation. This is independent of Rust stable handles.
        self.native_unique_ids.hash(hasher);
        self.hash_type_tech_levels(hasher);
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
        self.load_objects.hash_state(hasher);
        // Path searches waiting for the next frame's batch.
        if let Some(batch) = &self.path_batch {
            b"path-batch-v2".hash(hasher);
            batch.hash(hasher);
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
        if let Some(zones) = &self.zone_grid {
            b"retained-navigation-history-v1".hash(hasher);
            zones.fold_navigation_history(hasher);
        }
        // `resolved_terrain` is derived/skipped. Fold the exact saved real
        // CellClass `0x1180` values once through their serialized authority.
        b"real-cell-bridge-flags-v2".hash(hasher);
        self.real_cell_bridge_flags_0x1180.hash(hasher);
        b"dynamic-terrain-cells-v1".hash(hasher);
        self.dynamic_terrain_cells.hash(hasher);
        self.hash_overlay_grid(hasher);
        self.hash_crate_authority(hasher);
        self.hash_smudge_grid(hasher);
        self.hash_radiation(hasher);
        self.kamikaze.fold_hash(hasher);
        self.hash_projectiles(hasher);
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
        let bridge_keeps_dummy = shared_dummy_handle.native_anchor()
            == Some(crate::map::cell_index::NativeCellIdentity::Dummy);
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
        if bridge_keeps_dummy
            || cells_or_entities_retain_dummy
            || self
                .resolved_terrain
                .as_ref()
                .is_some_and(|terrain| self.team_script_vm.retains_dummy_cell(terrain))
            || self.projectiles.iter().any(|(_, projectile)| {
                projectile.target() == crate::sim::projectile::ProjectileTarget::DummyCell
            })
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
                if entity.patrol_post.cell() == Some(NativeCellIdentity::Dummy) {
                    return true;
                }
                [
                    entity.attack_target.target(),
                    entity.suspended_attack_target.target(),
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
            && self.playfield_revision == 0
        {
            // Preserve the historical headless-fixture stream while still
            // distinguishing every configured authority from absence.
            return;
        }
        b"playfield-authority-v2".hash(hasher);
        match self.playfield_bounds {
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
            projectile.collision().inaccurate.hash(hasher);
            projectile.collision().floater.hash(hasher);
            projectile.collision().elasticity_bits.hash(hasher);
            projectile.on_bridge().hash(hasher);
            projectile.collision().arcing.hash(hasher);
            if let Some(house) = projectile.firer_house() {
                b"bullet-firer-scheme-v1".hash(hasher);
                house.index().hash(hasher);
            }
            if projectile.awaiting_anim() {
                b"bullet-awaited-anim-v1".hash(hasher);
                projectile.awaited_anim().hash(hasher);
            }
        }
    }

    fn hash_waves(&self, hasher: &mut impl Hasher) {
        self.active_wave_links.len().hash(hasher);
        for (&owner_id, &wave_id) in &self.active_wave_links {
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
            .filter(|entity| entity.owner() == owner && !entity.kill_record.is_recorded())
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
        for (owner, house) in &self.houses {
            owner.hash(hasher);
            #[cfg(feature = "ares")]
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
            // House504080/503040 preserves the kill, loss and built totals
            // through native load. The retained-ship chain needs both its
            // initial and terminal loss. Preserve zero streams.
            if *house.stats() != crate::sim::house_state::MatchStatistics::default() {
                b"house-match-statistics-v1".hash(hasher);
                house.stats().hash(hasher);
            }
            house.side_index().hash(hasher);
            house.is_human().hash(hasher);
            if self.session.game_mode().is_network() {
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
            if !self.session.game_mode().is_network() {
                house.player_control().hash(hasher);
            }
            (house.difficulty() as i32).hash(hasher);
            house.rof_bias().bits().hash(hasher);
            house.is_defeated().hash(hasher);
            if house.is_to_die() {
                b"house-flag-to-die-v1".hash(hasher);
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
        let Some(bridge_state) = &self.bridge_state else {
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
        let Some(overlay_grid) = &self.overlay_grid else {
            0u8.hash(hasher);
            return;
        };
        1u8.hash(hasher);
        overlay_grid.width().hash(hasher);
        overlay_grid.height().hash(hasher);
        // Row-major order carries the coordinates.
        state_hasher::hash_records(overlay_grid.cells(), hasher, OverlayCell::hash_record);
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
        for slot in self.crate_authority.slots() {
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
        let Some(grid) = &self.smudge_grid else {
            0u8.hash(hasher);
            return;
        };
        1u8.hash(hasher);
        let mut entries: Vec<_> = grid
            .iter_occupied()
            .map(|(rx, ry, c)| (rx, ry, c.type_id, c.footprint_origin, c.frame_offset))
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
        let storm_touched = self.lightning_storm != Default::default();
        storm_touched.hash(hasher);
        if storm_touched {
            self.lightning_storm.hash(hasher);
        }
        // The Psychic Dominator's globals; the tag keeps a match that never
        // launched one on its established stream.
        if self.psychic_dominator != Default::default() {
            b"psychic-dominator-v1".hash(hasher);
            self.psychic_dominator.hash(hasher);
        }
    }

    /// One digest per entity of `entities`, the stored entities in stable-ID
    /// order (the store iterates in that order, so no sort is needed),
    /// spread over threads in large worlds.
    fn entity_digests(&self, entities: &[&GameEntity]) -> Vec<u64> {
        let frame = self.session.binary_frame();
        let network = self.session.game_mode().is_network();
        state_hasher::digests(entities, entities.len() >= 4096, |entity, hasher| {
            hash_entity(entity, frame, hasher);
            if network {
                entity.discovery.hash_network(hasher);
            }
        })
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
    p.direction().map(|v| v.to_bits()).hash(hasher);
    p.motion().map(|v| v.bits()).hash(hasher);
    p.velocity().bits().hash(hasher);
    p.damage_counter().hash(hasher);
    p.state_ai_advance().hash(hasher);
    (p.drift_x(), p.drift_y(), p.drift_z()).hash(hasher);
    p.prev_delta().map(|v| v.to_bits()).hash(hasher);
    match p.spark() {
        None => 0_u8.hash(hasher),
        Some(spark) => {
            1_u8.hash(hasher);
            spark.velocity_x.bits().hash(hasher);
            spark.velocity_y.bits().hash(hasher);
            spark.velocity_z.bits().hash(hasher);
            spark.start_rgb.hash(hasher);
            spark.color_index.hash(hasher);
            spark.color_accumulator.bits().hash(hasher);
        }
    }
}

/// One entity's components, in the order the state hash folds them.
/// `frame` is the binary frame the pending timers measure against.
fn hash_entity(entity: &GameEntity, frame: u32, hasher: &mut StateHasher) {
    entity.stable_id().hash(hasher);
    if entity.production_start_price() != 0 {
        // Techno+300 survives factory release and can suppress FreeUnit at
        // Grand_Opening446AF2. Native ComputeCRC70C3C6 also consumes it.
        b"production-start-price".hash(hasher);
        entity.production_start_price().hash(hasher);
    }
    if entity.is_useless() {
        // Native Techno+3D0 affects a later Repair-depot sale (`0x0044C4BC`).
        b"techno-useless-v1".hash(hasher);
    }
    // Shared count presence and the raw Techno reload clock affect future
    // firing/reload, including a signed -1 reached from InitialAmmo=-2.
    b"techno-ammo-reload-v1".hash(hasher);
    entity.ammo().is_some().hash(hasher);
    if let Some(ammo) = entity.ammo() {
        if entity.category() == crate::map::entities::EntityCategory::Aircraft {
            b"aircraft-ammo-v170".hash(hasher);
            ammo.hash(hasher);
        } else {
            ammo.current().hash(hasher);
        }
    }
    entity.ammo_reload_timer().hash(hasher);
    // GSI-09.01: the `BuildingClass+0x6D0/+0x6D8` ProduceCash
    // timer and the `TechnoClass+0x1CC/+0x1D0` drain link pair.
    // All three drive future wallet writes, so a divergence here
    // desyncs credits; every object carries them.
    entity.produce_cash.timer().hash(hasher);
    entity.drain.target().hash(hasher);
    entity.drain.draining_me().hash(hasher);
    // ParasiteClass (Foot+69C) of dogs and drones, the victim's
    // Foot+694/+698 links, and Foot+6A0 once it has been armed.
    // `limbo_reselect` (+432) is deliberately absent: it is set
    // only for the local player's selection, so peers differ.
    let parasite = &entity.parasite;
    if let Some(class) = parasite.class() {
        b"parasite-v193".hash(hasher);
        class.hash(hasher);
    }
    if parasite.eater().is_some() || parasite.launch_lock() != 0 {
        b"parasite-victim-v193".hash(hasher);
        parasite.eater().hash(hasher);
        parasite.launch_lock().hash(hasher);
    }
    if parasite.paralysis().duration() != 0 {
        0x6a0_u32.hash(hasher);
        parasite.paralysis().hash(hasher);
    }
    // Foot+5C4..+5D1, an attack-move order: its scan, resume and
    // pointer expiry (`sim::mission::mega_mission`).
    if entity.mega_mission != Default::default() {
        0x5c4_u32.hash(hasher);
        entity.mega_mission.hash(hasher);
    }
    if entity.has_been_captured.is_set() {
        // Building+6E3: doubles the survivor divisor, widens the
        // survivor roll and skips the engineer roll at death.
        0x6e3_u32.hash(hasher);
    }
    if entity.no_survivor.is_set() {
        // Building+6E0 (CRC `0x00454407`): no survivor leaves the building
        // that outlived its killing blow.
        0x6e0_u32.hash(hasher);
    }
    if entity.is_nominal_survivor() {
        //Infantry CRC521CE5..521CEE includes its retained6D9 byte.
        0x6d9_u32.hash(hasher);
    }
    if let Some(production) = entity.secret_lab_fallback() {
        b"building-secret-production-v382".hash(hasher);
        production.hash(hasher);
    }
    if entity.object_group() != -1 {
        // gamemd.exe TechnoClass::ComputeCRC70C270 reads +214 at70C280.
        // Replay state includes it; native MP checksum64DAB0 is separate and
        // omits local control groups, as in the original executable.
        b"techno-object-group-v1".hash(hasher);
        entity.object_group().hash(hasher);
    }
    if entity.techno_ctor_random_word() != 0 {
        b"techno-constructor-v1".hash(hasher);
        entity.techno_ctor_random_word().hash(hasher);
        // Preserve the previous no-upgrade hash stream.
        Option::<u8>::None.hash(hasher);
    }
    if entity.building_upgrades().count() != 0
        || entity
            .building_upgrades()
            .types()
            .iter()
            .any(Option::is_some)
    {
        b"building-installed-upgrades-v458".hash(hasher);
        entity.building_upgrades().hash(hasher);
    }
    // One retained native+F8/+FC/+100/+108/+10C/+110 clock, shared
    // by Infantry, harvest/unload and Building animation receivers.
    b"techno-stage-v264".hash(hasher);
    entity.native_stage().hash(hasher);
    entity.native_crush_immunity().hash(hasher);
    b"building-body-v277".hash(hasher);
    entity.hash_building_body(hasher);
    entity.hash_building_health_sample(hasher);
    if let Some(pending) = entity.pending_entry() {
        b"foot-pending-entry-500".hash(hasher);
        pending.hash(hasher);
    }
    if entity.building_entry.seeking() {
        b"foot-building-entry-68f-690".hash(hasher);
        entity.building_entry.hash(hasher);
    }
    let latches = entity.house_tracking_latches();
    #[cfg(feature = "phobos")]
    {
        b"phobos-techno-placement-v1".hash(hasher);
        entity.phobos_placement.hash(hasher);
        if let Some(shield) = &entity.shield {
            b"phobos-shield-v1".hash(hasher);
            shield.hash(hasher);
        }
    }
    if entity.tracking_facts.naval_combatant() {
        // Retained across reload: later Remove_Tracking must use the same
        // Naval/Passengers admission as construction, even while in limbo.
        b"naval-tracking-eligibility-v1".hash(hasher);
    }
    entity.tracking_facts.hash_retained_inputs(hasher);
    if latches != Default::default() {
        b"house-tracking-infantry-latches-v1".hash(hasher);
        latches.hash(hasher);
    }
    entity.native_unique_id().hash(hasher);
    if let Some(manager) = entity.slave_manager.as_ref() {
        b"slave-manager-v209".hash(hasher);
        manager.hash(hasher);
    }
    if entity.slave.owner().is_some() || !entity.slave.cargo().is_empty() {
        b"slave-v209".hash(hasher);
        entity.slave.owner().hash(hasher);
        entity.slave.cargo().len().hash(hasher);
        for bale in entity.slave.cargo() {
            bale.tiberium_type.0.hash(hasher);
            (bale.resource_type as u8).hash(hasher);
            bale.value.hash(hasher);
        }
    }
    entity.air_spatial_bucket().hash(hasher);
    entity.air_spatial_enter_order().hash(hasher);
    // Foot+560/+564 are independent retained native Cells. Constructor
    // NativeNull contributes no bytes, like the other sparse native
    // fields; either non-null cache independently affects continuation.
    if entity.air_tracker_cell() != (0, 0) {
        b"foot-air-tracker-cell-v289".hash(hasher);
        entity.air_tracker_cell().hash(hasher);
    }
    if entity.air_slot_cell() != (0, 0) {
        b"foot-air-slot-cell-v289".hash(hasher);
        entity.air_slot_cell().hash(hasher);
    }
    // Independent lifecycle axes and deterministic Rust bookkeeping.
    // Keep this order fixed: it is part of the lockstep hash contract.
    entity.lifecycle.object_alive().hash(hasher);
    entity.lifecycle.in_limbo().hash(hasher);
    entity.lifecycle.cell_marked().hash(hasher);
    entity.death.is_dying().hash(hasher);
    entity.removal_facts.dirty_rect_eligible().hash(hasher);
    entity.kill_record.is_recorded().hash(hasher);
    // TechnoClass+0x3D5 is mutable admission state, not a derived
    // position query: ordinary movement is promote-only while
    // teleport and Set_Clipped_LocalSize own exact demotions.
    entity.in_playfield().hash(hasher);
    entity.move_sound.is_active().hash(hasher);
    entity.crash.is_crashing().hash(hasher);
    entity.crash.seen().hash(hasher);
    if !entity.sinking.is_default() {
        b"techno-sinking-v1".hash(hasher);
        entity.sinking.hash(hasher);
    }
    entity.is_mission_only().hash(hasher);
    entity.move_sound.countdown().hash(hasher);
    entity.position.rx().hash(hasher);
    entity.position.ry().hash(hasher);
    entity.position.z().hash(hasher);
    // Most objects have no exact coordinate-Z override. Preserve their
    // pre-v62 hash stream while making TubeMovement's signed lepton Z
    // authoritative whenever it is present.
    if let Some(exact_z_leptons) = entity.position.exact_z_leptons() {
        b"exact-object-z-leptons-v1".hash(hasher);
        exact_z_leptons.hash(hasher);
    }
    entity.position.sub_x().hash(hasher);
    entity.position.sub_y().hash(hasher);
    // The body FacingClass (`+0x388`), each Techno's one heading.
    entity.body_facing.hash(hasher);
    // The barrel elevation (`+0x370`), tagged, once an Unlimbo moved
    // it off its constructor value.
    if !entity.barrel_elevation_is_constructed() {
        b"barrel-elevation-v1".hash(hasher);
        entity.barrel_elevation().hash(hasher);
    }
    entity.body_frame_counter.get().hash(hasher);
    if entity.unit_firing_counter.get() != -1 {
        b"unit-firing-counter-v1".hash(hasher);
        entity.unit_firing_counter.hash(hasher);
    }
    if !entity.tesla_charge.is_default() {
        b"building-tesla-charge-v1".hash(hasher);
        entity.tesla_charge.hash(hasher);
    }
    if entity.tesla_charger.is_active() {
        b"foot-tesla-charger-v1".hash(hasher);
    }
    // Building+6E6 is retained transition state, independent of HP.
    if entity.building_art.damage_state_active() {
        b"building-damage-state-v1".hash(hasher);
    }
    if entity.building_art.actually_placed()
        || entity.building_art.anim_slots().iter().any(Option::is_some)
        || (entity.building_art.anim_effect_replay().iter().any(|v| *v))
    {
        b"building-anim-slots-v1".hash(hasher);
        entity.building_art.actually_placed().hash(hasher);
        entity.building_art.anim_slots().hash(hasher);
        entity.building_art.anim_effect_replay().hash(hasher);
    }
    if *entity.building_art.storage() != Default::default() {
        b"building-storage-v1".hash(hasher);
        entity.building_art.storage().hash(hasher);
    }
    if entity.building_last_operational.is_operational() {
        b"building-operational-v1".hash(hasher);
    }
    if !entity.building_art.stuff_enabled() {
        b"building-stuff-disabled-v1".hash(hasher);
    }
    if entity.category() == crate::map::entities::EntityCategory::Structure
        && !entity.building_online()
    {
        b"building-online-latch-off-v1".hash(hasher);
    }
    if entity.building_has_engineer.is_set() {
        b"building-has-engineer-v1".hash(hasher);
    }
    // Native Building ComputeCRC4542E0 includes the retained +53C value,
    // not merely the -1 loss gate. Keep the constructor-zero stream stable.
    if entity.category() == crate::map::entities::EntityCategory::Structure
        && entity.building_loss_index() != 0
    {
        b"building-loss-index-v1".hash(hasher);
        entity.building_loss_index().hash(hasher);
    }
    if !entity.unit_delivery_provides_base_space() {
        b"unit-delivery-no-base-v1".hash(hasher);
    }
    // Building+662 and Techno+1C8 (`sim::powered_unit`), folded only
    // while set.
    if entity.building_powered_center.is_counted() {
        b"building-powered-center-v1".hash(hasher);
    }
    if entity.deactivated.is_offline() {
        b"techno-deactivated-v1".hash(hasher);
    }
    entity.hash_emp_state(hasher);
    #[cfg(feature = "ares")]
    entity.hash_attach_effects(hasher);
    // Infantry gameplay reads its class-owned Doing/Stage fields.
    // A restored presentation component is not a second state owner.
    if entity.category() != crate::map::entities::EntityCategory::Infantry
        && let Some(animation) = entity.animation.as_ref()
    {
        b"entity-animation-v1".hash(hasher);
        animation.sequence().hash(hasher);
        animation.frame_index().hash(hasher);
        animation.elapsed_frames().hash(hasher);
        animation.finished().hash(hasher);
    }
    entity.owner().hash(hasher);
    #[cfg(feature = "ares")]
    if let Some(country) = entity.original_factory_country() {
        b"ares-original-factory-country-v1".hash(hasher);
        country.hash(hasher);
    }
    // Techno+14C survives ChangeOwner and controls an AI building capture's
    // automatic sale. Preserve the untouched-owner stream, but distinguish
    // objects transferred from different constructor houses.
    if entity.initial_owner() != entity.owner() {
        b"techno-initial-owner-v1".hash(hasher);
        entity.initial_owner().hash(hasher);
    }
    entity.health.current().hash(hasher);
    entity.estimated_health.get().hash(hasher);
    entity.type_ref().hash(hasher);
    (entity.category() as u8).hash(hasher);
    // Keep the stock name encoding used before Foundation became an enum.
    // Custom values still fold their full dimensions, cells and exit outline.
    let foundation = entity.type_facts.foundation();
    match foundation {
        crate::rules::foundation::Foundation::Stock(_) => foundation.name().hash(hasher),
        #[cfg(feature = "ares")]
        _ => foundation.hash(hasher),
    }
    entity.type_facts.building_hidden_occupancy().hash(hasher);
    entity.type_facts.base_reservation_spacing().hash(hasher);
    if entity.type_facts.build_const_eligible() {
        b"naval-build-const-entity-v1".hash(hasher);
        entity.type_facts.build_const_eligible().hash(hasher);
    }
    entity.type_facts.base_plan_type_index().hash(hasher);
    entity.type_facts.base_plan_is_defense().hash(hasher);
    entity
        .type_facts
        .base_plan_has_undeploy_target()
        .hash(hasher);
    // The rank is sampled from the raw accumulator; its fold keeps its
    // place in the stream. The accumulator itself is folded too, so
    // two objects one kill apart inside the same rank are distinct.
    entity.veterancy.rank().hash(hasher);
    entity.veterancy.raw().bits().hash(hasher);
    entity.veterancy.rank_cache().hash(hasher);
    // The flash countdown (+F0) is not folded: TechnoClass::Flash's gate and
    // the MegaMission event's flash read the current player, so lockstep
    // peers hold different values, and no gameplay decision reads it.
    entity.crate_multipliers.armor().bits().hash(hasher);
    // Folded only once a Firepower crate changed it, so the legacy
    // hash stream of every untouched object is preserved.
    if entity.crate_multipliers.crate_firepower() != crate::util::native_x87::NativeF64Bits::ONE {
        b"firepower-mult-v1".hash(hasher);
        entity
            .crate_multipliers
            .crate_firepower()
            .bits()
            .hash(hasher);
    }
    #[cfg(feature = "ares")]
    if entity.crate_multipliers.firepower() != entity.crate_multipliers.crate_firepower() {
        b"attached-firepower-v1".hash(hasher);
        entity.crate_multipliers.firepower().bits().hash(hasher);
    }
    entity.berserk.hash(hasher);
    entity.ai_sell_repair.was_attacked_by_enemy().hash(hasher);
    if entity.category() == crate::map::entities::EntityCategory::Structure {
        entity.ai_sell_repair.ai_sellable().hash(hasher);
    }
    if entity.category() == crate::map::entities::EntityCategory::Structure {
        entity.repairing.is_on().hash(hasher);
        entity.ai_sell_repair.ai_repairable().hash(hasher);
    }
    let ai_placement_timer = entity.ai_placement_timer.timer();
    if ai_placement_timer != crate::sim::timer::CdTimer::default() {
        b"ai-placement-timer-v1".hash(hasher);
        ai_placement_timer.hash(hasher);
    }
    b"base-defense-response-v1".hash(hasher);
    entity.base_defense_response.hash(hasher);
    entity.mover_facts.regular_crusher().hash(hasher);
    entity.mover_facts.drive_accelerates().hash(hasher);
    entity.damage_fires.is_active().hash(hasher);
    entity.damage_fires.anim_ids().hash(hasher);
    entity.sight.range().hash(hasher);
    entity.sight_refresh_timers.hash(hasher);
    if entity.gap_generator != crate::sim::world::GapGeneratorRuntime::default() {
        b"gap-operational-v144".hash(hasher);
        entity.gap_generator.hash(hasher);
    }

    {
        // Preserve schema160's original four-field order. Deriving
        // the fold from the expanded struct would alter every prior
        // projection, including constructor-default Foot states.
        let path = entity.navigation.path_runtime();
        path.movement_timer().hash(hasher);
        path.blocked_timer().hash(hasher);
        path.path_blocked().hash(hasher);
        path.retries_left().hash(hasher);
        if path.scold_latch_raw() != 0 {
            0x68a_u32.hash(hasher);
            path.scold_latch_raw().hash(hasher);
        }
    }
    // Foot+530 is retained across type changes and saved restores.
    // Preserve the constructor-zero historical projection; nonzero
    // values (including negative zero) change future path decisions.
    let coefficient = entity.navigation.path_threat_coefficient();
    if coefficient.bits() != 0 {
        0x530_u32.hash(hasher);
        coefficient.hash(hasher);
    }
    // Techno+508 is a retained contribution, not recomputed live
    // ThreatPosed. Some(0) differs from constructor-uninitialized None:
    // a later removal must not read an invented constructor value.
    if let Some(value) = entity.cached_spatial_threat() {
        b"techno-spatial-threat-v1".hash(hasher);
        value.hash(hasher);
    }
    entity.navigation.neighbor_state().hash(hasher);
    entity.navigation.path_replay().hash(hasher);
    entity.navigation.nav_com_aux().hash(hasher);
    entity.navigation.nav_com().hash(hasher);
    entity.navigation.suspended_nav_com().hash(hasher);
    entity.navigation.nav_queue().hash(hasher);
    // Foot+5AC/+6B1 hash only when used, so established goldens without a
    // navigation list keep their values.
    if !entity.navigation.nav_list().is_empty() || entity.navigation.nav_list_loops() {
        0x5ac_u32.hash(hasher);
        entity.navigation.nav_list().hash(hasher);
        entity.navigation.nav_list_loops().hash(hasher);
    }
    entity.navigation.pending_destination_setter().hash(hasher);

    entity.foot_speed.native_applied_fraction().hash(hasher);
    if entity.foot_speed.crush_slowdown() {
        0x6b5_u32.hash(hasher);
        true.hash(hasher);
    }
    if entity.flight_attitude != Default::default() {
        0x2e8_u32.hash(hasher);
        entity.flight_attitude.hash(hasher);
    }
    entity.foot_speed.crate_multiplier().hash(hasher);
    entity.foot_occupation_enabled().hash(hasher);
    entity.lift.swap_active().hash(hasher);
    // The Magnetron's links and drop (Techno+2AC/+2B0/+427/+428) and
    // the warp or drop credit house (Techno+42C) are clear outside a
    // lift or a warp; Foot+6AE stays up after any Jumpjet touchdown
    // until an END clears it. Tagged like +0x1F8 below.
    if entity.lift.may_end() {
        0x6ae_u32.hash(hasher);
    }
    if let Some(target) = entity.lift.target() {
        0x2ac_u32.hash(hasher);
        target.hash(hasher);
    }
    if let Some(source) = entity.lift.source() {
        0x2b0_u32.hash(hasher);
        source.hash(hasher);
    }
    if entity.drop_credit.being_manipulated() {
        0x427_u32.hash(hasher);
    }
    if let Some(firer) = entity.drop_credit.manipulated_by() {
        0x428_u32.hash(hasher);
        firer.hash(hasher);
    }
    if let Some(house) = entity.drop_credit.warped_by_house() {
        0x42c_u32.hash(hasher);
        house.hash(hasher);
    }
    // Techno+0x1F8 is up only between the Teleporter arm's can't-end
    // branch and the next Unit setter call; the tag keeps every
    // established stream unchanged while it is clear.
    if entity.setter_force_reassign.is_raised() {
        0x1f8_u32.hash(hasher);
    }
    // Techno+0x284 stays zero until a blocked Chronosphere landing
    // writes it; tagged like +0x1F8.
    if entity.chrono_warp_delay() != 0 {
        0x284_u32.hash(hasher);
        entity.chrono_warp_delay().hash(hasher);
    }

    if let Some(ref loco) = entity.locomotor {
        1u8.hash(hasher);
        hash_locomotor(loco, hasher);
        // Mission readiness inputs are NOT hashed: they are derived at
        // the gate from state already hashed above (and from position and
        // facing, likewise hashed). Hashing a derived predicate would pin a
        // projection of the same state twice, and it cannot diverge
        // independently of its inputs.
    } else {
        0u8.hash(hasher);
    }

    entity.on_bridge().hash(hasher);
    entity
        .runtime_bridge_transition
        .pending_mismatch()
        .hash(hasher);
    entity.building_light.spotlight_capable().hash(hasher);
    entity.building_light.light().hash(hasher);
    entity.low_bridge_tube_state.hash(hasher);
    if let Some(cloak) = entity.cloak.as_ref() {
        1u8.hash(hasher);
        cloak.state().hash(hasher);
        cloak.depth().hash(hasher);
        cloak.step_delta().hash(hasher);
        cloak.step_timer().timer().start_frame().hash(hasher);
        cloak.step_timer().speed().hash(hasher);
        cloak.step_timer().timer().duration().hash(hasher);
        cloak.recloak_delay().start_frame().hash(hasher);
        cloak.recloak_delay().duration().hash(hasher);
    } else {
        0u8.hash(hasher);
    }
    if let Some(deposit) = entity.sensor_deposit {
        1u8.hash(hasher);
        deposit.owner().hash(hasher);
        deposit.center().hash(hasher);
        deposit.add_radius().hash(hasher);
        deposit.remove_radius().hash(hasher);
        deposit.building_array().hash(hasher);
        // The cached `DetectDisguiseRange=` circle. It is the
        // radius Limbo will decrement, so it selects which
        // cells leave the disguise-detect plane.
        deposit.detect_disguise_radius().hash(hasher);
    } else {
        0u8.hash(hasher);
    }
    if let Some(disguise) = entity.disguise.as_ref() {
        1u8.hash(hasher);
        disguise.hash_state(hasher);
    } else {
        0u8.hash(hasher);
    }

    if let Some(ref inv) = entity.invulnerability {
        1u8.hash(hasher);
        inv.hash_state(hasher);
    } else {
        0u8.hash(hasher);
    }

    if let Some(target) = entity.attack_target.target() {
        1u8.hash(hasher);

        target.hash(hasher);
    } else {
        0u8.hash(hasher);
    }
    entity.rearm_timer.start_frame().hash(hasher);
    entity.rearm_timer.duration().hash(hasher);
    entity.weapon_burst.hash(hasher);
    entity.pending_building_fire.pending().hash(hasher);
    if entity.prism_support_count.count() != 0 {
        b"prism-support-count-v1".hash(hasher);
        entity.prism_support_count.count().hash(hasher);
    }
    // One native weapon owner replaces last-shot/transport copies.
    // The turret index and saved charge duration only drive drawing.
    entity.current_weapon_number().hash(hasher);

    // Slot-indexed fold: capacity + each slot's Option (null holes and
    // pad positions are hash-relevant). Replaces the old len + ordered-id
    // fold — an intended one-time re-baseline at this behavior boundary.
    entity.radio_contacts.hash_fold(hasher);
    // Dock-entered flag (+0x418 analogue). Intended one-time re-baseline
    // at this behavior boundary alongside the slot-folded contacts.
    match entity.tether.partner() {
        Some(sid) => {
            1u8.hash(hasher);
            sid.hash(hasher);
        }
        None => 0u8.hash(hasher),
    }

    entity.hash_capture_infantry_type(hasher);
    match entity.pending_c4_detonation.pending() {
        Some(pending) => {
            true.hash(hasher);
            pending.timer().remaining(frame as i32).hash(hasher);
            pending.source_entity_id().hash(hasher);
        }
        None => false.hash(hasher),
    }
    entity.bunker.occupant().hash(hasher);
    // Reciprocal link + install machine are authoritative lifecycle state.
    entity.bunker.link().hash(hasher);
    entity.bunker_runtime.hash(hasher);
    if let Some(gate) = entity.building_gate {
        1u8.hash(hasher);
        gate.hash_state(hasher);
    } else {
        0u8.hash(hasher);
    }
    // Shared Techno Door gameplay state is retained and folded once.
    entity.hash_door_state(hasher);

    // Unit+68C remains the deployment authority. The former MCV-only
    // turn observation now hashes once in its retained locomotor class.
    if entity.mcv_deploy_pending.is_pending() {
        0x4d435644u32.hash(hasher);
        entity.mcv_deploy_pending.is_pending().hash(hasher);
    }
    entity.hash_deploy_attachment(hasher);
    entity.hash_behind_anim(hasher);

    if let Some(infantry) = entity.infantry {
        1u8.hash(hasher);
        // Infantry5220F0's retained6D8 changes target admission. Preserve
        // established streams until this separate latch actually rises.
        if infantry.is_psycho() {
            0x36D8_u32.hash(hasher);
        }
        infantry.fear_level().hash(hasher);
        infantry.is_prone().hash(hasher);
        // The idle-fidget countdown gates a scenario-RNG draw, so a
        // divergence here becomes a divergence of every later draw.
        // Folded in every schema variant rather than behind a gate: the
        // Scenario RNG itself is folded before either gate, and the two
        // provenance fixtures both hold infantry eligible on their first
        // tick, so their legacy probes already move with the draws this
        // timer schedules. Gating it would hide the field without
        // buying those probes back.
        infantry.idle_action_timer().hash(hasher);
        // Infantry+6DC is written only by the failed-path receiver
        // 0x51DAF0; the tagged extension keeps every established
        // stream unchanged until a receiver actually stores 1.
        if infantry.cell_entry_blocked() {
            0x36DC_u32.hash(hasher);
        }
    } else {
        0u8.hash(hasher);
    }

    if let Some(ref miner) = entity.miner {
        1u8.hash(hasher);
        // The FSM cursor retired from this block at the substate-
        // authority flip: it is MissionCom.handler_state, folded by
        // `hash_mission_com` (and the pre-v29 reconstruction).
        (miner.kind() as u8).hash(hasher);
        (miner.cargo().len() as u16).hash(hasher);
        for bale in miner.cargo() {
            bale.tiberium_type.0.hash(hasher);
            (bale.resource_type as u8).hash(hasher);
            bale.value.hash(hasher);
        }
        let retired_dock_fold = false;
        if retired_dock_fold {
            // home_refinery: None.
            None::<u64>.hash(hasher);
        }
        // The retired return-order refinery; nothing writes it.
        miner.reserved_refinery().hash(hasher);
        let native_ore_field = true;
        if !native_ore_field {
            // The retired target_ore_cell (None) and harvest_timer
            // (unarmed).
            None::<(u16, u16)>.hash(hasher);
            crate::sim::mission::MissionTimer::default().hash(hasher);
        }
        if native_ore_field {
            // Unit+0x6D1/+0x6D2; the shared Stage is folded above.
            miner.unload_active().hash(hasher);
            miner.harvesting().hash(hasher);
        }
        if retired_dock_fold {
            // dock_queued false, dock_phase Approach (discriminant 0),
            // dock_pivot_facing None.
            false.hash(hasher);
            0_isize.hash(hasher);
            None::<crate::sim::movement::FacingClass>.hash(hasher);
        }
    } else {
        0u8.hash(hasher);
    }

    // Passenger/transport state.
    let role = &entity.passenger_role;
    match role.cargo() {
        Some(cargo) => {
            1u8.hash(hasher);
            cargo.capacity().hash(hasher);
            (cargo.passengers().len() as u32).hash(hasher);
            for &pid in cargo.passengers() {
                pid.hash(hasher);
            }
            debug_assert_eq!(cargo.passenger_sizes().len(), cargo.passengers().len());
            for &passenger_size in cargo.passenger_sizes() {
                passenger_size.to_bits().hash(hasher);
            }
            cargo.total_size().hash(hasher);
            // A Foot can own cargo while riding another transport (Unit
            // PerCell 0x0073A730..0x0073A77C). Only append a live parent
            // relation, preserving ordinary outside-cargo hash controls.
            if let Some((parent, open_topped)) = role.inside_transport() {
                3u8.hash(hasher);
                parent.hash(hasher);
                open_topped.hash(hasher);
            }
            // `UnitClass+0x6E4` keep-count, written only by a
            // transport's own Unload state 0. Folded only when set so
            // objects that never unloaded (every pinned harness
            // object) keep their v-schema bytes.
            if entity.transport_unload_keep_count.count() != 0 {
                entity.transport_unload_keep_count.count().hash(hasher);
            }
        }
        None => {
            if let Some((transport_id, open_topped)) = role.inside_transport() {
                3u8.hash(hasher);
                transport_id.hash(hasher);
                // `+0x82`, folded only when set so a closed transport's
                // cargo keeps its historical hash.
                if open_topped {
                    1u8.hash(hasher);
                }
            } else {
                0u8.hash(hasher);
            }
        }
    }
    // BuildingCtor43B8DC..43B907 and GetCount4581F0: independent vector,
    // not Cargo+114. The unique empty/zero constructor state adds no bytes,
    // preserving unrelated replay controls. A nonzero signed +69C still
    // contributes even when pointer expiry has emptied the vector.
    let occupants = role.occupants();
    if entity.category() == crate::map::entities::EntityCategory::Structure
        && (!occupants.is_empty() || occupants.fire_index() != 0)
    {
        b"building-occupants-694-69c".hash(hasher);
        occupants.count().hash(hasher);
        occupants.ids().hash(hasher);
        occupants.fire_index().hash(hasher);
    }
    // Spawn-manager pool: slot states, timers and targets are
    // deterministic sim state that no other field covers. (Native
    // folds only the manager-level fields into its CRC and leaves the
    // per-slot machine uncovered; VERA folds the whole thing, which is
    // strictly stricter and cannot mask a divergence.)
    //
    // Deliberately folded ONLY when present — no absent-case tag byte.
    // Every object in the game carries these two fields, so an
    // unconditional tag would move every committed baseline, including
    // the legacy provenance probes, for fixtures that contain no
    // spawner unit at all.
    //
    // Honest limitation: this block is not self-delimiting. The leading
    // 1u8/2u8 tags separate the two fields from each other, but nothing
    // in this hasher marks where one entity's contribution ends, so an
    // omitted-field encoding is not provably distinct from some other
    // field's bytes further along the stream. That is a property of the
    // whole per-entity hasher, not of this block; every neighbouring
    // conditional field has it too. It is not reachable here — a live
    // pool always folds a tag plus its spawn type and mode — but the
    // invariant is "no known aliasing", not "aliasing is impossible".
    entity.airstrike.hash_state(hasher);
    entity.airstrike_tint.hash_state(hasher);
    if entity.type_facts.spawned_non_missile_aircraft() {
        b"spawned-non-missile-aircraft".hash(hasher);
    }
    if let Some(ref manager) = entity.spawn_manager {
        1u8.hash(hasher);
        manager.hash(hasher);
    }
    if let Some(owner_id) = entity.spawn_owner_id.owner() {
        2u8.hash(hasher);
        owner_id.hash(hasher);
    }
    // CaptureManager capacity and MCNode order gate future fire/mission
    // decisions. As with SpawnManager, absent managers add no bytes so
    // worlds without a mind-control controller keep legacy hashes.
    if let Some(ref manager) = entity.capture_manager {
        3u8.hash(hasher);
        manager.hash(hasher);
    }
    if entity.mind_control != crate::sim::capture_manager::MindControlLink::default() {
        // The victim's MindControlledBy (+2C0) and ring (+2C8).
        0x2c0_u32.hash(hasher);
        entity.mind_control.hash(hasher);
    }
    // TemporalImUsing (+274) and TemporalTargetingMe (+278). Objects
    // with neither fold nothing, so worlds without a Temporal firer
    // keep their hashes.
    if entity.temporal != crate::sim::temporal::TemporalState::default() {
        0x274_u32.hash(hasher);
        entity.temporal.hash(hasher);
    }
    // A carried bomb (+38); an unbombed object folds nothing.
    if let Some(bomb) = &entity.bomb {
        0x38_u32.hash(hasher);
        bomb.hash(hasher);
    }
    // Gattling stage, value and report latch (+140, +144, +4B8); an
    // object that never spun folds nothing.
    if entity.gattling != crate::sim::combat::gattling::GattlingState::default() {
        0x140_u32.hash(hasher);
        entity.gattling.hash(hasher);
    }
    // Turret rotation-sound latch (`+0x49C`): unarmed suppresses another
    // playback request until idle, even if no source exists. No other
    // folded field reproduces that history. The armed default folds
    // nothing, so a building that never turned keeps its prior hash stream.
    if !entity.turret_rotate_latch.is_armed() {
        b"turret-rotate-latch-v1".hash(hasher);
    }
    // Techno+4F8 retains a future ordinary voice suppression which
    // neither the mission nor the audio pending line reconstructs.
    if entity.deploy_voice_suppressed() {
        b"deploy-voice-suppression-v1".hash(hasher);
    }
    // Retired slot: the entity homing model had no production creator
    // and is gone; its constant absent tag keeps pinned hashes stable.
    0u8.hash(hasher);
    // Turret facing (`+0x3A0`) — Hash-derived, all primitive fields
    // contribute.
    if let Some(barrel) = entity.barrel_facing.as_ref() {
        1u8.hash(hasher);
        barrel.hash(hasher);
    } else {
        0u8.hash(hasher);
    }

    // Body rocking's four authoritative binary32 fields. A sub-fixed-quantum
    // rate can select a different damping branch on the next visit, so the
    // presentation's fixed getters cannot stand in for retained state.
    // Drive/Ship slope state is hashed with its locomotor payload below.
    if let Some(ref r) = entity.rocking {
        1u8.hash(hasher);
        r.native_bits().hash(hasher);
    } else {
        0u8.hash(hasher);
    }

    hash_mission_com(&entity.mission, hasher);
    hash_mission_leaf(&entity.mission_leaf, hasher);
    if let Some(post) = entity.patrol_post.cell() {
        b"foot-patrol-post-v1".hash(hasher);
        post.hash(hasher);
    }
    entity.removal_facts.occupier().hash(hasher);
    entity.passive_scan.timer().hash(hasher);
    // Passive-acquire bookkeeping. `passively_acquired_target` gates
    // the stale-target drop and the off-mission clear, so a
    // divergence here changes future targets; the scan-frame stamp
    // rides along in the same block.
    entity.passive_scan.last_scan_frame().hash(hasher);
    entity.passive_scan.acquired().hash(hasher);
    entity.hash_distributed_fire(hasher);
    // Foot688 changes the next scan's topology/radius. Constructor
    // false preserves earlier streams; the retained true state is an
    // independent schema255 contribution, never inferred from TarCom.
    if entity.foot_retarget_after_stop() {
        0x004D_9920_u32.hash(hasher);
        true.hash(hasher);
    }
    match entity.suspended_attack_target.target() {
        Some(target) => {
            1u8.hash(hasher);
            target.hash(hasher);
        }
        None => 0u8.hash(hasher),
    }
    // Preserve the idle hash while retaining every nonzero fall word.
    // Landing retains the rate/+84/+8F for a later DropAsBomb.
    if entity.is_falling_down()
        || entity.fall_rate() != 0
        || entity.is_parachuted()
        || entity.was_dropped_as_bomb()
    {
        1u8.hash(hasher);
        entity.is_falling_down().hash(hasher);
        entity.fall_rate().hash(hasher);
        entity.is_parachuted().hash(hasher);
        entity.was_dropped_as_bomb().hash(hasher);
    } else {
        0u8.hash(hasher);
    }

    // Preserve the constructor-NULL representation while hashing the actual
    // retained system identity, which gates FireAt/GetROF and future AI draws.
    entity
        .spark_particle_system
        .system()
        .unwrap_or(0)
        .hash(hasher);
    // ReceiveDamage damage-Smoke `+0x310` identity. This gates later
    // spawn/RNG and remains set while a marked system drains.
    entity.damage_smoke_system_id.system().hash(hasher);
}

/// One locomotor object, field for field, and the object its piggyback slot
/// suspends through the same fold. [`LocomotorHashFields`] names every field
/// (its builder destructures the object), and `deny(unused_variables)` makes a
/// named but unfolded field a compile error; `locomotor_field_hash_tests`
/// checks that each one reaches the hash.
///
/// [`LocomotorHashFields`]: crate::sim::movement::locomotor::LocomotorHashFields
#[deny(unused_variables)]
fn hash_locomotor(
    loco: &crate::sim::movement::locomotor::LocomotorState,
    hasher: &mut impl Hasher,
) {
    let crate::sim::movement::locomotor::LocomotorHashFields {
        kind,
        powered,
        piggyback,
        runtime_payload,
        layer,
        balloon_hover,
        speed_type,
        movement_zone,
    } = loco.hash_fields();
    // The installed class is the kind of the bottom object, which this
    // fold reaches through the stash.
    (kind as u8).hash(hasher);
    powered.hash(hasher);
    (layer as u8).hash(hasher);
    balloon_hover.hash(hasher);
    speed_type.hash(hasher);
    movement_zone.hash(hasher);
    hash_locomotor_payload(runtime_payload, hasher);
    match piggyback {
        Some(stashed) => {
            1u8.hash(hasher);
            hash_locomotor(stashed, hasher);
        }
        None => 0u8.hash(hasher),
    }
}

fn hash_locomotor_payload(
    payload: &crate::sim::movement::locomotion::piggyback::LocomotorRuntimePayload,
    hasher: &mut impl Hasher,
) {
    use crate::sim::movement::locomotion::piggyback::LocomotorRuntimePayload;
    match payload {
        LocomotorRuntimePayload::Drive(state) => {
            0u8.hash(hasher);
            hash_slope_transition_state(state.slope(), hasher);
            state.retained().hash(hasher);
        }
        LocomotorRuntimePayload::Walk(state) => {
            1u8.hash(hasher);
            state.hash(hasher);
        }
        LocomotorRuntimePayload::Teleport(state) => {
            2u8.hash(hasher);
            state.hash(hasher);
        }
        LocomotorRuntimePayload::Rocket(state) => {
            4u8.hash(hasher);
            state.hash(hasher);
        }
        LocomotorRuntimePayload::Hover(state) => {
            6u8.hash(hasher);
            state.hash(hasher);
        }
        LocomotorRuntimePayload::Ship(state) => {
            8u8.hash(hasher);
            hash_slope_transition_state(state.slope(), hasher);
            state.retained().hash(hasher);
        }
        LocomotorRuntimePayload::Fly(state) => {
            9u8.hash(hasher);
            let (target_height, taking_off, landing) = state.height_hash_fields();
            target_height.hash(hasher);
            taking_off.hash(hasher);
            landing.hash(hasher);
            state.destination().hash(hasher);
            state.cruise_mode().hash(hasher);
            state.moving().hash(hasher);
            state.landing_effect_latched().hash(hasher);
            state.airport_bound().hash(hasher);
            state.fall_counter().hash(hasher);
            state.target_speed().bits().hash(hasher);
            state.current_speed().bits().hash(hasher);
        }
        LocomotorRuntimePayload::Jumpjet(state) => {
            10u8.hash(hasher);
            state.hash(hasher);
        }
    }
}

fn hash_slope_transition_state(
    state: &crate::sim::movement::slope_transition::SlopeTransitionState,
    hasher: &mut impl Hasher,
) {
    // Native Drive/Ship raw-block persistence includes these defined fields;
    // Load does not resample (`Save` 0x004AF800/0x0069EF10, shared raw writer
    // 0x0055AA60). Hash every defined field for active and stash.
    let (previous_slope, current_slope, start_frame, transition_total) = state.hash_fields();
    previous_slope.hash(hasher);
    current_slope.hash(hasher);
    start_frame.hash(hasher);
    transition_total.hash(hasher);
}

#[cfg(test)]
mod raw_cell_occupation_hash_tests {
    use super::Simulation;

    #[test]
    fn gsi_04_12_raw_occupation_hash_distinguishes_byte_plane_and_coordinate() {
        let empty = Simulation::new();

        let mut ground = Simulation::new();
        ground.substrate.raw_cell_occupation.mark_ground(4, 9, 0x20);

        let mut different_byte = Simulation::new();
        different_byte
            .substrate
            .raw_cell_occupation
            .mark_ground(4, 9, 0x40);

        let mut deck = Simulation::new();
        deck.substrate.raw_cell_occupation.mark_deck(4, 9, 0x20);

        let mut different_coordinate = Simulation::new();
        different_coordinate
            .substrate
            .raw_cell_occupation
            .mark_ground(9, 4, 0x20);

        assert_ne!(empty.state_hash(), ground.state_hash());
        assert_ne!(ground.state_hash(), different_byte.state_hash());
        assert_ne!(ground.state_hash(), deck.state_hash());
        assert_ne!(ground.state_hash(), different_coordinate.state_hash());
    }

    #[test]
    fn gsi_04_12_raw_occupation_hash_is_insertion_order_independent() {
        let mut forward = Simulation::new();
        forward
            .substrate
            .raw_cell_occupation
            .mark_ground(2, 7, 0x04);
        forward.substrate.raw_cell_occupation.mark_deck(8, 3, 0x80);

        let mut reverse = Simulation::new();
        reverse.substrate.raw_cell_occupation.mark_deck(8, 3, 0x80);
        reverse
            .substrate
            .raw_cell_occupation
            .mark_ground(2, 7, 0x04);

        assert_eq!(forward.state_hash(), reverse.state_hash());
    }
}

#[cfg(test)]
mod overlay_grid_hash_tests {
    use super::Simulation;
    use crate::map::overlay::OverlayDataPack;
    use crate::sim::miner::ResourceType;
    use crate::sim::overlay_grid::OverlayGrid;

    #[test]
    fn gsi_04_09_empty_overlay_raw_data_changes_state_hash() {
        let mut sim_a = Simulation::new();
        let mut sim_b = Simulation::new();
        sim_a.overlay_grid = Some(OverlayGrid::from_overlay_packs(
            &[],
            &OverlayDataPack::from_cells([]),
            2,
            2,
        ));
        sim_b.overlay_grid = Some(OverlayGrid::from_overlay_packs(
            &[],
            &OverlayDataPack::from_cells([(1, 1, 42)]),
            2,
            2,
        ));

        let a = sim_a.overlay_grid.as_ref().expect("overlay grid A");
        let b = sim_b.overlay_grid.as_ref().expect("overlay grid B");
        assert_eq!(a.cell(1, 1).overlay_id, None);
        assert_eq!(b.cell(1, 1).overlay_id, None);
        assert_eq!(a.cell(1, 1).overlay_data, 0);
        assert_eq!(b.cell(1, 1).overlay_data, 42);
        assert!(a.iter_occupied().next().is_none());
        assert!(b.iter_occupied().next().is_none());
        assert_ne!(sim_a.state_hash(), sim_b.state_hash());
    }

    #[test]
    fn retained_wall_neighbor_plane_changes_hash_with_identical_final_cells() {
        let cells = vec![(-1, 0), (1, 2), (-1, 0), (-1, 0)];
        let mut sim_a = Simulation::new();
        let mut sim_b = Simulation::new();
        sim_a.overlay_grid = Some(OverlayGrid::from_finalized_map_payload(
            crate::map::authored_overlay::FinalizedOverlayPayload::from_cells_for_test(
                2,
                2,
                cells.clone(),
                vec![0, 1, 0, 0],
            ),
        ));
        sim_b.overlay_grid = Some(OverlayGrid::from_finalized_map_payload(
            crate::map::authored_overlay::FinalizedOverlayPayload::from_cells_for_test(
                2,
                2,
                cells,
                vec![0, 2, 0, 0],
            ),
        ));

        for y in 0..2 {
            for x in 0..2 {
                assert_eq!(
                    sim_a.overlay_grid.as_ref().expect("A").cell(x, y),
                    sim_b.overlay_grid.as_ref().expect("B").cell(x, y)
                );
            }
        }
        assert_ne!(sim_a.state_hash(), sim_b.state_hash());
    }

    #[test]
    fn a_new_grid_hashes_like_a_finalized_all_zero_wall_plane() {
        let mut fresh = Simulation::new();
        fresh.overlay_grid = Some(OverlayGrid::new(2, 2));

        let mut finalized = Simulation::new();
        finalized.overlay_grid = Some(OverlayGrid::from_finalized_map_payload(
            crate::map::authored_overlay::FinalizedOverlayPayload::from_cells_for_test(
                2,
                2,
                vec![(-1, 0); 4],
                vec![0; 4],
            ),
        ));

        assert_eq!(
            fresh.state_hash(),
            finalized.state_hash(),
            "every grid retains its wall plane, all-zero included"
        );
    }

    #[test]
    fn tiberium_cells_are_hashed_by_cell_not_placement_order() {
        use crate::sim::tiberium::test_support::place_tiberium;

        let mut forward = Simulation::new();
        place_tiberium(&mut forward, 8, 3, ResourceType::Gem, 3);
        place_tiberium(&mut forward, 2, 7, ResourceType::Ore, 3);

        let mut reverse = Simulation::new();
        place_tiberium(&mut reverse, 2, 7, ResourceType::Ore, 3);
        place_tiberium(&mut reverse, 8, 3, ResourceType::Gem, 3);
        assert_eq!(
            forward.state_hash(),
            reverse.state_hash(),
            "the overlay grid is folded in cell order, not placement order"
        );

        place_tiberium(&mut reverse, 8, 3, ResourceType::Gem, 4);
        assert_ne!(forward.state_hash(), reverse.state_hash());
        place_tiberium(&mut reverse, 8, 3, ResourceType::Ore, 3);
        assert_ne!(forward.state_hash(), reverse.state_hash());
    }
}

#[cfg(test)]
mod lifecycle_hash_tests {
    use super::Simulation;
    use crate::sim::game_entity::GameEntity;

    fn assert_entity_mutation_changes_hash(mutate: impl FnOnce(&mut GameEntity)) {
        let mut sim = Simulation::new();
        sim.substrate
            .entities
            .insert(GameEntity::test_default(1, "MTNK", "Americans", 5, 5));
        let before = sim.state_hash();
        mutate(sim.substrate.entities.get_mut(1).expect("fixture entity"));
        assert_ne!(before, sim.state_hash());
    }

    #[cfg(feature = "ares")]
    #[test]
    fn custom_foundation_geometry_changes_state_hash_despite_equal_names() {
        let rows = crate::rules::foundation::custom_tests::native_rows();
        let first = crate::rules::foundation::custom_tests::rules(&rows[0])
            .object("CUSTOM_TEST")
            .unwrap()
            .foundation
            .clone();
        let second = crate::rules::foundation::custom_tests::rules(&rows[1])
            .object("CUSTOM_TEST")
            .unwrap()
            .foundation
            .clone();
        assert_eq!(first.name(), second.name());
        let mut entity = GameEntity::test_default_of_category(
            1,
            "CUSTOM_TEST",
            "Americans",
            5,
            5,
            crate::map::entities::EntityCategory::Structure,
        );
        entity.type_facts.set_foundation_for_test(first);
        let mut sim = Simulation::new();
        sim.substrate.entities.insert(entity);
        let before = sim.state_hash();
        sim.substrate
            .entities
            .get_mut(1)
            .unwrap()
            .type_facts
            .set_foundation_for_test(second);
        assert_ne!(before, sim.state_hash());
    }

    #[test]
    fn constructor_owner_distinguishes_otherwise_identical_transferred_entities() {
        use crate::map::entities::EntityCategory;
        use crate::sim::intern::test_intern;

        let hash_for = |initial: &str| {
            let mut sim = Simulation::new();
            sim.substrate
                .entities
                .insert(GameEntity::test_default_of_category(
                    1,
                    "GAPOWR",
                    initial,
                    5,
                    5,
                    EntityCategory::Structure,
                ));
            sim.substrate
                .entities
                .change_owner(1, test_intern("Russians"));
            sim.state_hash()
        };
        // All three objects now belong to Russia. Constructor ownership still
        // changes what a later capture back to America will do.
        let american = hash_for("Americans");
        let neutral = hash_for("Neutral");
        let russian = hash_for("Russians");
        assert_ne!(american, neutral);
        assert_ne!(american, russian);
        assert_ne!(neutral, russian);
    }

    #[test]
    fn lifecycle_authority_axes_and_pending_queue_change_state_hash() {
        {
            assert_entity_mutation_changes_hash(|entity| {
                entity
                    .lifecycle
                    .set_object_alive_for_test(!entity.lifecycle.object_alive());
            });
            assert_entity_mutation_changes_hash(|entity| {
                entity
                    .lifecycle
                    .set_in_limbo_for_test(!entity.lifecycle.in_limbo());
            });
            assert_entity_mutation_changes_hash(|entity| {
                entity
                    .lifecycle
                    .set_cell_marked_for_test(!entity.lifecycle.cell_marked());
            });
            assert_entity_mutation_changes_hash(|entity| {
                entity.death.set_dying_for_test(!entity.death.is_dying());
            });
            assert_entity_mutation_changes_hash(|entity| {
                entity
                    .removal_facts
                    .set_dirty_rect_eligible_for_test(!entity.removal_facts.dirty_rect_eligible());
            });
            assert_entity_mutation_changes_hash(|entity| {
                entity
                    .kill_record
                    .set_destruction_recorded_for_test(!entity.kill_record.is_recorded());
            });
            assert_entity_mutation_changes_hash(|entity| {
                entity
                    .removal_facts
                    .set_occupier_for_test(!entity.removal_facts.occupier());
            });
            assert_entity_mutation_changes_hash(|entity| {
                entity.set_in_playfield(!entity.in_playfield());
            });
            assert_entity_mutation_changes_hash(|entity| {
                entity.passive_scan.timer_mut_for_test().arm(7, 12);
            });
        }
        {
            let mut empty = Simulation::new();
            let empty_hash = empty.state_hash();

            empty.substrate.pending_delete.queue_object(1);
            let one_hash = empty.state_hash();
            empty.substrate.pending_delete.queue_object(2);
            let ordered_hash = empty.state_hash();
            empty.substrate.pending_delete.swap_for_test(0, 1);
            let reversed_hash = empty.state_hash();

            assert_ne!(empty_hash, one_hash);
            assert_ne!(one_hash, ordered_hash);
            assert_ne!(ordered_hash, reversed_hash);
        }
    }
}

#[cfg(test)]
mod mission_authority_hash_tests {
    use super::Simulation;
    use crate::sim::combat::TargetKind;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::mission::leaf::MissionLeafState;
    use crate::sim::mission::state::MissionTestFixture;
    use crate::sim::mission::{MissionDispatchTimer, MissionId};

    fn hash_entity(entity: GameEntity) -> u64 {
        let mut sim = Simulation::new();
        sim.substrate.entities.insert(entity);
        sim.state_hash()
    }

    fn hash_mission(fixture: MissionTestFixture) -> u64 {
        let mut entity = GameEntity::test_default(1, "MTNK", "Americans", 5, 5);
        entity.mission.apply_test_fixture(fixture);
        hash_entity(entity)
    }

    fn hash_leaf(leaf: MissionLeafState) -> u64 {
        let mut entity = GameEntity::test_default(1, "MTNK", "Americans", 5, 5);
        entity.mission_leaf = leaf;
        hash_entity(entity)
    }

    fn hash_suspended_target(target: Option<TargetKind>) -> u64 {
        let mut entity = GameEntity::test_default(1, "MTNK", "Americans", 5, 5);
        entity.suspended_attack_target.set_for_test(target);
        hash_entity(entity)
    }

    #[test]
    fn every_mission_com_raw_field_changes_state_hash() {
        let base = MissionTestFixture {
            current: MissionId::from_raw(0x1234_5678),
            suspended: MissionId::from_raw(i32::MIN),
            queued: MissionId::from_raw(i32::MAX),
            movement_bypass_latch: 0xa5,
            handler_state: 0x1122_3344,
            mission_start_frame: 0x5566_7788,
            ai_counter: 0x99aa_bbcc,
            dispatch_timer: MissionDispatchTimer::from_raw(-17, -29),
        };
        let base_hash = hash_mission(base);
        let variants = [
            (
                "current raw unknown selector",
                MissionTestFixture {
                    current: MissionId::from_raw(0x1234_5679),
                    ..base
                },
            ),
            (
                "suspended raw unknown selector",
                MissionTestFixture {
                    suspended: MissionId::from_raw(i32::MIN + 1),
                    ..base
                },
            ),
            (
                "queued raw unknown selector",
                MissionTestFixture {
                    queued: MissionId::from_raw(i32::MAX - 1),
                    ..base
                },
            ),
            (
                "movement bypass latch",
                MissionTestFixture {
                    movement_bypass_latch: 0xa4,
                    ..base
                },
            ),
            (
                "handler state",
                MissionTestFixture {
                    handler_state: 0x1122_3345,
                    ..base
                },
            ),
            (
                "mission start frame",
                MissionTestFixture {
                    mission_start_frame: 0x5566_7789,
                    ..base
                },
            ),
            (
                "AI counter",
                MissionTestFixture {
                    ai_counter: 0x99aa_bbcd,
                    ..base
                },
            ),
            (
                "dispatch timer start frame",
                MissionTestFixture {
                    dispatch_timer: MissionDispatchTimer::from_raw(-18, -29),
                    ..base
                },
            ),
            (
                "dispatch timer delay",
                MissionTestFixture {
                    dispatch_timer: MissionDispatchTimer::from_raw(-17, -30),
                    ..base
                },
            ),
        ];

        for (field, variant) in variants {
            assert_ne!(
                base_hash,
                hash_mission(variant),
                "{field} must contribute to the state hash"
            );
        }
    }

    #[test]
    fn every_class_mission_leaf_field_changes_state_hash() {
        {
            let base = MissionLeafState::unit_raw_for_test(1, 2, 3, 4);
            let base_hash = hash_leaf(base);
            for (field, variant) in [
                (
                    "deploy begin active",
                    MissionLeafState::unit_raw_for_test(5, 2, 3, 4),
                ),
                (
                    "deploy reverse active",
                    MissionLeafState::unit_raw_for_test(1, 5, 3, 4),
                ),
                (
                    "tracker byte 18",
                    MissionLeafState::unit_raw_for_test(1, 2, 5, 4),
                ),
                (
                    "tracker byte 19",
                    MissionLeafState::unit_raw_for_test(1, 2, 3, 5),
                ),
            ] {
                assert_ne!(
                    base_hash,
                    hash_leaf(variant),
                    "Unit {field} must contribute to the state hash"
                );
            }
        }
        {
            let base = MissionLeafState::infantry_raw_for_test(7, 12);
            let base_hash = hash_leaf(base);
            let mut water = base;
            water.set_infantry_water_state(false);
            let mut land = base;
            land.set_infantry_water_state(true);
            for (field, variant) in [
                (
                    "firing sequence latch",
                    MissionLeafState::infantry_raw_for_test(8, 12),
                ),
                ("Doing", MissionLeafState::infantry_raw_for_test(7, 13)),
                ("water state", water),
                ("land state", land),
            ] {
                assert_ne!(
                    base_hash,
                    hash_leaf(variant),
                    "Infantry {field} must contribute to the state hash"
                );
            }
            assert_ne!(hash_leaf(water), hash_leaf(land));
        }
        {
            let base = MissionLeafState::aircraft_raw_for_test(9, 10);
            let base_hash = hash_leaf(base);
            for (field, variant) in [
                (
                    "action latch",
                    MissionLeafState::aircraft_raw_for_test(10, 10),
                ),
                (
                    "transition ready latch",
                    MissionLeafState::aircraft_raw_for_test(9, 11),
                ),
            ] {
                assert_ne!(
                    base_hash,
                    hash_leaf(variant),
                    "Aircraft {field} must contribute to the state hash"
                );
            }
        }
        {
            assert_ne!(
                hash_leaf(MissionLeafState::building_raw_for_test(0)),
                hash_leaf(MissionLeafState::building_raw_for_test(1)),
                "Building ready latch must contribute to the state hash"
            );
        }
    }

    #[test]
    fn suspended_attack_target_presence_variant_and_payloads_change_state_hash() {
        let absent = hash_suspended_target(None);
        let entity_seven = hash_suspended_target(Some(TargetKind::Entity(7)));
        assert_ne!(
            absent, entity_seven,
            "suspended target presence must contribute to the state hash"
        );
        assert_ne!(
            entity_seven,
            hash_suspended_target(Some(TargetKind::Entity(8))),
            "suspended Entity target ID must contribute to the state hash"
        );

        let cell = hash_suspended_target(Some(TargetKind::Cell(10, 20)));
        assert_ne!(
            entity_seven, cell,
            "suspended target variant must contribute to the state hash"
        );
        assert_ne!(
            cell,
            hash_suspended_target(Some(TargetKind::Cell(11, 20))),
            "suspended Cell target X must contribute to the state hash"
        );
        assert_ne!(
            cell,
            hash_suspended_target(Some(TargetKind::Cell(10, 21))),
            "suspended Cell target Y must contribute to the state hash"
        );
    }

    #[test]
    fn falling_state_and_its_fall_rate_change_state_hash() {
        let base = GameEntity::test_default(1, "MTNK", "Americans", 5, 5);
        let mut falling = base.clone();
        falling.set_falling_down_for_test(true);
        let mut faster = falling.clone();
        faster.set_fall_rate_for_test(-2);

        let falling_hash = hash_entity(falling);
        assert_ne!(hash_entity(base), falling_hash, "IsFallingDown");
        assert_ne!(falling_hash, hash_entity(faster), "fall rate");
    }
}

#[cfg(test)]
mod track_authority_hash_tests {
    use super::Simulation;
    use crate::rules::locomotor_type::LocomotorKind;
    use crate::sim::components::{DriveCoord, TrackProgress};
    use crate::sim::game_entity::GameEntity;
    use crate::sim::movement::locomotor::LocomotorState;
    use crate::sim::movement::track_process::TrackFamily;
    use crate::sim::movement::{DriveLocomotionRuntime, ShipLocomotionRuntime};

    fn supplied_track_world(ship: bool, active: bool) -> Simulation {
        let mut sim = Simulation::new();
        let mut entity = GameEntity::test_default(1, "MTNK", "Americans", 10, 10);
        let track = if active {
            {
                let mut value = TrackProgress::default();
                value.set_turn_index(0);
                value.set_cursor(0);
                value
            }
        } else {
            TrackProgress::default()
        };
        let head_to = active.then_some(DriveCoord::cell(10, 9, 0));
        let mut loco = LocomotorState::for_test_kind(if ship {
            LocomotorKind::Ship
        } else {
            LocomotorKind::Drive
        });
        let family = TrackFamily::from_kind(loco.active_kind()).unwrap();
        loco.ensure_installed_track_state();
        loco.store_track_progress(family, track);
        loco.store_track_head(family, head_to);
        loco.store_track_valid(family, active);
        entity.locomotor = Some(loco);
        sim.substrate.entities.insert(entity);
        sim
    }

    #[test]
    fn current_hash_covers_each_retained_track_progress_field() {
        for (ship, suspended) in [(false, false), (false, true), (true, false), (true, true)] {
            for field in 0..6 {
                let mut sim = supplied_track_world(ship, true);
                if suspended {
                    assert!(
                        sim.substrate
                            .entities
                            .get_mut(1)
                            .unwrap()
                            .locomotor
                            .as_mut()
                            .unwrap()
                            .begin_piggyback(LocomotorKind::Teleport, 0)
                    );
                }
                let before = sim.state_hash();
                let entity = sim.substrate.entities.get_mut(1).unwrap();
                let family = if ship {
                    TrackFamily::Ship
                } else {
                    TrackFamily::Drive
                };
                let loco = entity.locomotor.as_mut().unwrap();
                if field == 5 {
                    loco.store_track_turn_latched(
                        family,
                        !loco.track_turn_latched(family).unwrap(),
                    );
                } else if field == 4 {
                    loco.store_track_valid(family, !loco.track_valid(family).unwrap());
                } else {
                    let mut track = loco.track_progress(family).unwrap();
                    match field {
                        0 => track.set_turn_index(1),
                        1 => track.set_cursor(1),
                        2 => track.set_reversed(true),
                        3 => track.set_residual(1),
                        _ => unreachable!(),
                    }
                    loco.store_track_progress(family, track);
                }
                assert_ne!(
                    before,
                    sim.state_hash(),
                    "ship={ship} suspended={suspended} field={field}"
                );
            }
        }
    }

    #[test]
    fn current_hash_covers_retained_coordinates_speed_and_permission() {
        let point = DriveCoord::cell(3, 4, 731);
        let drives = [
            DriveLocomotionRuntime::default().with_destination_for_test(Some(point)),
            DriveLocomotionRuntime::default().with_head_to_for_test(Some(point)),
            DriveLocomotionRuntime::default()
                .with_target_speed_fraction_for_test(crate::util::fixed_math::SIM_HALF),
            DriveLocomotionRuntime::default().with_end_permitted_for_test(false),
        ];
        let ships = [
            ShipLocomotionRuntime::default().with_destination_for_test(Some(point)),
            ShipLocomotionRuntime::default().with_head_to_for_test(Some(point)),
            ShipLocomotionRuntime::default()
                .with_target_speed_fraction_for_test(crate::util::fixed_math::SIM_HALF),
        ];
        for (ship, suspended) in [(false, false), (false, true), (true, false), (true, true)] {
            let base = supplied_track_world(ship, false);
            let mut empty = base;
            if suspended {
                assert!(
                    empty
                        .substrate
                        .entities
                        .get_mut(1)
                        .unwrap()
                        .locomotor
                        .as_mut()
                        .unwrap()
                        .begin_piggyback(LocomotorKind::Teleport, 0)
                );
            }
            let original = empty.state_hash();
            for field in 0..if ship { ships.len() } else { drives.len() } {
                let mut changed = Simulation::new();
                changed
                    .substrate
                    .entities
                    .insert(empty.substrate.entities.get(1).unwrap().clone());
                let loco = changed
                    .substrate
                    .entities
                    .get_mut(1)
                    .unwrap()
                    .locomotor
                    .as_mut()
                    .unwrap();
                assert!(if ship {
                    loco.install_ship_state_for_test(Some(ships[field].clone()))
                } else {
                    loco.install_drive_state_for_test(Some(drives[field].clone()))
                });
                assert_ne!(
                    original,
                    changed.state_hash(),
                    "ship={ship} suspended={suspended} retained field={field}"
                );
            }
            let loco = empty
                .substrate
                .entities
                .get_mut(1)
                .unwrap()
                .locomotor
                .as_mut()
                .unwrap();
            assert!(if ship {
                loco.install_ship_state_for_test(None)
            } else {
                loco.install_drive_state_for_test(None)
            });
            assert_ne!(
                original,
                empty.state_hash(),
                "absence must differ from a present default instance"
            );
        }
    }
}

#[cfg(test)]
mod state_hash_field_tests {
    use super::Simulation;
    use crate::sim::game_entity::GameEntity;

    #[test]
    fn drive_accelerates_changes_state_hash() {
        let mut sim_a = Simulation::new();
        let mut sim_b = Simulation::new();
        let entity_a = GameEntity::test_default(1, "GTNK", "Americans", 10, 10);
        let mut entity_b = entity_a.clone();
        entity_b.mover_facts.set_drive_accelerates_for_test(false);
        sim_a.substrate.entities.insert(entity_a);
        sim_b.substrate.entities.insert(entity_b);

        assert_ne!(sim_a.state_hash(), sim_b.state_hash());
    }

    #[test]
    fn house_fields_change_state_hash() {
        use crate::sim::house_state::{HouseDifficulty, HouseState};

        {
            let mut sim_a = Simulation::new();
            let mut sim_b = Simulation::new();
            let owner_a = sim_a.interner.intern("Computer1");
            let owner_b = sim_b.interner.intern("Computer1");
            assert_eq!(owner_a, owner_b);
            sim_a
                .houses
                .register(HouseState::new(owner_a, 0, None, false, 0, 10));
            let mut hard_house = HouseState::new(owner_b, 0, None, false, 0, 10);
            hard_house.set_difficulty_for_test(HouseDifficulty::Hard);
            sim_b.houses.register(hard_house);

            assert_ne!(sim_a.state_hash(), sim_b.state_hash());
        }
        {
            // Same stored difficulty, different bias: only the game-mode arm of
            // `set_difficulty` folds the country's ROF into `HouseClass+0x1A8`.
            let hashes: Vec<u64> = [false, true]
                .into_iter()
                .map(|game_mode_nonzero| {
                    let mut sim = Simulation::new();
                    let owner = sim.interner.intern("Computer1");
                    let mut house = HouseState::new(owner, 0, None, false, 0, 10);
                    let mut general = crate::rules::ruleset::GeneralRules::default();
                    general.difficulty_rows =
                        [0.8, 1.0, 1.2].map(|rof| crate::rules::ruleset::DifficultyRules {
                            rof,
                            ..Default::default()
                        });
                    house.set_difficulty(
                        HouseDifficulty::Hard,
                        &general,
                        crate::rules::ruleset::CountryDifficultyBiases {
                            rof: crate::util::native_x87::NativeF64Bits::from_bits(
                                0.9_f64.to_bits(),
                            ),
                            ..Default::default()
                        },
                        game_mode_nonzero,
                        0,
                        0,
                    );
                    sim.houses.register(house);
                    sim.state_hash()
                })
                .collect();
            assert_ne!(hashes[0], hashes[1]);
        }
        {
            let mut baseline = Simulation::new();
            let mut changed = Simulation::new();
            let owner = baseline.interner.intern("Computer1");
            let changed_owner = changed.interner.intern("Computer1");
            assert_eq!(owner, changed_owner);
            let mut house = HouseState::new(owner, 0, None, false, 0, 10);
            house.set_base_center_for_test(Some((41, 52)));
            let mut changed_house = house.clone();
            changed_house.set_alternate_base_center_for_test((93, 106));
            baseline.houses.register(house);
            changed.houses.register(changed_house);

            assert_ne!(baseline.state_hash(), changed.state_hash());
            assert_eq!(
                baseline.houses[&owner].base_center(),
                changed.houses[&owner].base_center()
            );
        }
        {
            let mut sim_a = Simulation::new();
            let mut sim_b = Simulation::new();
            let owner_a = sim_a.interner.intern("Computer1");
            let owner_b = sim_b.interner.intern("Computer1");
            assert_eq!(owner_a, owner_b);
            sim_a
                .houses
                .register(HouseState::new(owner_a, 0, None, false, 0, 10));
            let mut changed = HouseState::new(owner_b, 0, None, false, 0, 10);
            changed.base_reservation_mut().update_bounds(3, 4, 5, 6);
            changed
                .base_reservation_mut()
                .append_perimeter_cell_if_absent(u32::from(3u16) | (u32::from(4u16) << 16));
            sim_b.houses.register(changed);

            assert_ne!(sim_a.state_hash(), sim_b.state_hash());
        }
        {
            fn fixture() -> (Simulation, crate::sim::intern::InternedId) {
                let mut sim = Simulation::new();
                let owner = sim.interner.intern("Computer1");
                sim.houses
                    .register(HouseState::new(owner, 0, None, false, 0, 10));
                (sim, owner)
            }

            let (baseline, _) = fixture();
            let baseline_hash = baseline.state_hash();

            let (mut mode, owner) = fixture();
            mode.houses
                .get_mut(&owner)
                .unwrap()
                .strategy_emergency_mut()
                .set_state_four();
            assert_ne!(baseline_hash, mode.state_hash(), "mode is hashed");

            let (mut bias, owner) = fixture();
            bias.houses
                .get_mut(&owner)
                .unwrap()
                .strategy_emergency_mut()
                .set_all_to_hunt_bias();
            assert_ne!(baseline_hash, bias.state_hash(), "bias latch is hashed");

            let (mut attack_frame, owner) = fixture();
            attack_frame
                .houses
                .get_mut(&owner)
                .unwrap()
                .strategy_emergency_mut()
                .note_building_attack(-17);
            assert_ne!(
                baseline_hash,
                attack_frame.state_hash(),
                "last Building attack frame is hashed"
            );

            let (mut attacker_index, owner) = fixture();
            attacker_index
                .houses
                .get_mut(&owner)
                .unwrap()
                .strategy_emergency_mut()
                .note_building_attacker(2);
            assert_ne!(
                baseline_hash,
                attacker_index.state_hash(),
                "last attacker House index is hashed"
            );

            let (mut timer, owner) = fixture();
            timer
                .houses
                .get_mut(&owner)
                .unwrap()
                .set_strategy_timer_for_test(crate::sim::timer::CdTimer::started(12, 106));
            assert_ne!(
                baseline_hash,
                timer.state_hash(),
                "Strategy timer is hashed"
            );
        }
        {
            let mut north = Simulation::new();
            let mut south = Simulation::new();
            let north_owner = north.interner.intern("Player");
            let south_owner = south.interner.intern("Player");
            assert_eq!(north_owner, south_owner);

            let mut north_house = HouseState::new(north_owner, 0, None, true, 0, 10);
            north_house.set_waypoint_edge_for_test(0);
            north.houses.register(north_house);

            let mut south_house = HouseState::new(south_owner, 0, None, true, 0, 10);
            south_house.set_waypoint_edge_for_test(2);
            south.houses.register(south_house);

            assert_ne!(north.state_hash(), south.state_hash());
        }
    }

    #[test]
    fn gsi_04_05_base_plan_state_and_entity_facts_are_current_schema_hash_authority() {
        use crate::sim::base_plan::{BasePlanNode, pack_base_plan_cell};
        use crate::sim::house_state::HouseState;

        fn house_sim(nodes: Vec<BasePlanNode>, percent_built: i32) -> Simulation {
            let mut sim = Simulation::new();
            let owner = sim.interner.intern("Computer1");
            let mut house = HouseState::new(owner, 0, None, false, 0, 10);
            house
                .base_plan_mut()
                .set_percent_built_for_test(percent_built);
            house.base_plan_mut().set_nodes_for_test(nodes);
            sim.houses.register(house);
            sim
        }

        let first = BasePlanNode {
            type_or_control: 4,
            packed_cell: pack_base_plan_cell(7, 8),
            filled: false,
            retry_count: 2,
        };
        let second = BasePlanNode {
            type_or_control: -3,
            packed_cell: pack_base_plan_cell(-1, 5),
            filled: true,
            retry_count: -9,
        };
        let baseline = house_sim(vec![first, second], 50);
        let reversed = house_sim(vec![second, first], 50);
        assert_ne!(baseline.state_hash(), reversed.state_hash());

        for changed in [
            house_sim(vec![first, second], 51),
            house_sim(
                vec![
                    BasePlanNode {
                        type_or_control: 5,
                        ..first
                    },
                    second,
                ],
                50,
            ),
            house_sim(
                vec![
                    BasePlanNode {
                        packed_cell: pack_base_plan_cell(9, 8),
                        ..first
                    },
                    second,
                ],
                50,
            ),
            house_sim(
                vec![
                    BasePlanNode {
                        filled: true,
                        ..first
                    },
                    second,
                ],
                50,
            ),
            house_sim(
                vec![
                    BasePlanNode {
                        retry_count: 3,
                        ..first
                    },
                    second,
                ],
                50,
            ),
        ] {
            assert_ne!(baseline.state_hash(), changed.state_hash());
        }

        let mut entity_a = Simulation::new();
        let mut entity_b = Simulation::new();
        let mut a = GameEntity::test_default(1, "GAPOWR", "Computer1", 10, 10);
        let mut b = a.clone();
        a.type_facts.set_base_plan_type_index_for_test(2);
        a.type_facts.set_base_plan_is_defense_for_test(true);
        a.type_facts
            .set_base_plan_has_undeploy_target_for_test(true);
        b.type_facts.set_base_plan_type_index_for_test(3);
        entity_a.substrate.entities.insert(a);
        entity_b.substrate.entities.insert(b);
        assert_ne!(entity_a.state_hash(), entity_b.state_hash());
    }

    #[test]
    fn house_ai_activation_hash_matches_native_direct_crc_fields_only() {
        use crate::sim::house_state::{HouseAiActivationLatches, HouseState};

        fn fixture(latches: HouseAiActivationLatches) -> Simulation {
            let mut sim = Simulation::new();
            let owner = sim.interner.intern("Computer1");
            let mut house = HouseState::new(owner, 0, None, false, 0, 10);
            house.set_ai_activation_for_test(latches);
            sim.houses.register(house);
            sim
        }

        let baseline = fixture(HouseAiActivationLatches::default());
        let production = fixture(HouseAiActivationLatches {
            production: true,
            autocreate_allowed: false,
            ai_triggers_active: false,
            auto_base_building: false,
        });
        let autocreate = fixture(HouseAiActivationLatches {
            production: false,
            autocreate_allowed: true,
            ai_triggers_active: false,
            auto_base_building: false,
        });
        let ai_triggers = fixture(HouseAiActivationLatches {
            production: false,
            autocreate_allowed: false,
            ai_triggers_active: true,
            auto_base_building: false,
        });
        let auto_base = fixture(HouseAiActivationLatches {
            production: false,
            autocreate_allowed: false,
            ai_triggers_active: false,
            auto_base_building: true,
        });

        assert_ne!(baseline.state_hash(), production.state_hash());
        assert_ne!(baseline.state_hash(), autocreate.state_hash());
        assert_ne!(baseline.state_hash(), ai_triggers.state_hash());
        assert_eq!(baseline.state_hash(), auto_base.state_hash());
    }

    #[test]
    fn gsi_04_05_techno_base_defense_state_changes_world_hash() {
        let mut baseline = Simulation::new();
        let entity = crate::sim::game_entity::GameEntity::test_default(1, "E1", "Computer1", 3, 4);
        baseline.substrate.entities.insert(entity.clone());
        let baseline_hash = baseline.state_hash();

        let mut changed = Simulation::new();
        let mut entity = entity;
        entity
            .base_defense_response
            .set_recruitable_b_for_test(false);
        entity.set_archive_target(Some(crate::sim::combat::TargetKind::Entity(7)));
        entity
            .base_defense_response
            .set_cooldown_for_test(crate::sim::timer::CdTimer::started(12, 225));
        changed.substrate.entities.insert(entity);
        assert_ne!(baseline_hash, changed.state_hash());
    }
}

#[cfg(test)]
mod particle_hash_tests {
    use super::Simulation;
    use crate::rules::particle_system_type::ParticleSystemTypeId;
    use crate::rules::particle_type::ParticleTypeId;
    use crate::sim::particles::{Particle, ParticleSystem, SparkRuntimeState};
    use crate::util::fixed_math::SimFixed;
    use crate::util::native_x87::{NativeF32Bits, NativeF64Bits};
    use glam::IVec3;

    fn fake_system(coords: IVec3) -> ParticleSystem {
        ParticleSystem::test_fixture(0, ParticleSystemTypeId(0), coords)
    }

    fn insert_system(sim: &mut Simulation, mut system: ParticleSystem) -> u64 {
        let id = sim.substrate.allocate_stable_id();
        system.set_stable_id_for_test(id);
        sim.substrate.particle_systems.insert(system);
        sim.reveal_particle_system(id, crate::rules::ruleset::RuleSet::empty_for_test());
        id
    }

    fn particle_with_spark(spark: Option<SparkRuntimeState>) -> Particle {
        let mut particle = Particle::test_fixture(ParticleTypeId(0), IVec3::new(-1, 2, 3));
        particle.set_origin_for_test(IVec3::ZERO);
        particle.set_lifetime_remaining_for_test(9);
        particle.set_spark_for_test(spark);
        particle
    }

    fn hash_with_particle(particle: Particle) -> u64 {
        let mut sim = Simulation::new();
        let mut system = fake_system(IVec3::ZERO);
        system.particles_mut_for_test().push(particle);
        insert_system(&mut sim, system);
        sim.state_hash()
    }

    #[test]
    fn every_raw_spark_field_changes_the_state_hash() {
        let base = SparkRuntimeState {
            velocity_x: NativeF32Bits::from_bits(0x0000_0000),
            velocity_y: NativeF32Bits::from_bits(0x3f80_0000),
            velocity_z: NativeF32Bits::from_bits(0xc0c0_0000),
            start_rgb: [80, 255, 255],
            color_index: 0,
            color_accumulator: NativeF64Bits::POSITIVE_ZERO,
        };
        let base_hash = hash_with_particle(particle_with_spark(Some(base)));
        let variants = [
            SparkRuntimeState {
                velocity_x: NativeF32Bits::NEGATIVE_ZERO,
                ..base
            },
            SparkRuntimeState {
                velocity_y: NativeF32Bits::from_bits(0x4000_0000),
                ..base
            },
            SparkRuntimeState {
                velocity_z: NativeF32Bits::from_bits(0xc100_0000),
                ..base
            },
            SparkRuntimeState {
                start_rgb: [255, 255, 100],
                ..base
            },
            SparkRuntimeState {
                color_index: -1,
                ..base
            },
            SparkRuntimeState {
                color_accumulator: NativeF64Bits::NEGATIVE_ZERO,
                ..base
            },
        ];
        for variant in variants {
            assert_ne!(
                base_hash,
                hash_with_particle(particle_with_spark(Some(variant)))
            );
        }
        assert_ne!(base_hash, hash_with_particle(particle_with_spark(None)));
    }

    fn hash_with_system(system: ParticleSystem) -> u64 {
        let mut sim = Simulation::new();
        insert_system(&mut sim, system);
        sim.state_hash()
    }

    /// Every saved system and particle field reaches the hash (#855).
    #[test]
    fn saved_system_and_particle_fields_change_the_state_hash() {
        let mut base = fake_system(IVec3::ZERO);
        base.particles_mut_for_test()
            .push(particle_with_spark(None));
        let base_hash = hash_with_system(base.clone());
        let owner_house = Simulation::new().interner.intern("Americans");
        type Edit = Box<dyn Fn(&mut ParticleSystem)>;
        let edits: Vec<Edit> = vec![
            Box::new(|s| s.set_offset_for_test(IVec3::new(1, 0, 0))),
            Box::new(|s| {
                s.set_spawn_timer_for_test(crate::util::native_x87::NativeF32Bits::from_bits(
                    0x4000_0000,
                ))
            }),
            Box::new(|s| s.set_spark_spawn_frames_for_test(1)),
            Box::new(|s| {
                s.set_target_ref_for_test(Some(crate::sim::combat::TargetKind::Entity(7)))
            }),
            Box::new(|s| s.set_owner_entity_for_test(Some(7))),
            Box::new(|s| s.set_target_coords_for_test(IVec3::new(0, 1, 0))),
            Box::new(move |s| s.set_owner_house_for_test(Some(owner_house))),
            Box::new(|s| s.particles_mut_for_test()[0].set_origin_for_test(IVec3::new(0, 0, 1))),
            Box::new(|s| {
                s.particles_mut_for_test()[0].direction_mut_for_test()[1] = SimFixed::from_num(1)
            }),
            Box::new(|s| {
                s.particles_mut_for_test()[0].motion_mut_for_test()[1] =
                    crate::util::native_x87::NativeF32Bits::ONE
            }),
            Box::new(|s| {
                s.particles_mut_for_test()[0]
                    .set_velocity_for_test(crate::util::native_x87::NativeF32Bits::ONE)
            }),
            Box::new(|s| s.particles_mut_for_test()[0].set_damage_counter_for_test(1)),
            Box::new(|s| s.particles_mut_for_test()[0].set_state_ai_advance_for_test(1)),
            Box::new(|s| s.particles_mut_for_test()[0].set_drift_x_for_test(1)),
            Box::new(|s| s.particles_mut_for_test()[0].set_drift_y_for_test(1)),
            Box::new(|s| s.particles_mut_for_test()[0].set_drift_z_for_test(1)),
            Box::new(|s| {
                s.particles_mut_for_test()[0].prev_delta_mut_for_test()[2] = SimFixed::from_num(1)
            }),
        ];
        for (index, edit) in edits.iter().enumerate() {
            let mut system = base.clone();
            edit(&mut system);
            assert_ne!(base_hash, hash_with_system(system), "edit {index}");
        }
    }

    #[test]
    fn terrain_spawner_active_fields_change_state_hash() {
        use crate::sim::terrain_spawn::TerrainAnimationState;

        let mut sim_a = Simulation::new();
        let mut sim_b = Simulation::new();
        let type_ref = sim_a.interner.intern("TIBTRE01");
        let state = TerrainAnimationState::new(
            type_ref,
            crate::util::native_x87::NativeF32Bits::ONE,
            3,
            22,
            0,
        );
        sim_a
            .terrain_objects
            .animations_mut_for_test()
            .insert(10, state.clone());
        sim_b
            .terrain_objects
            .animations_mut_for_test()
            .insert(10, state);
        assert_eq!(sim_a.state_hash(), sim_b.state_hash());

        let spawner_b = sim_b
            .terrain_objects
            .animations_mut_for_test()
            .get_mut(&10)
            .unwrap();
        spawner_b.advance_for_test(0, &mut crate::sim::rng::SimRng::new(0));
        assert_ne!(
            sim_a.state_hash(),
            sim_b.state_hash(),
            "all terrain spawner state fields must affect state hash",
        );
    }
}

#[cfg(test)]
mod radio_contact_hash_tests {
    use super::Simulation;
    use crate::map::entities::EntityCategory;
    use crate::sim::combat::Health;
    use crate::sim::game_entity::GameEntity;

    fn vehicle_entity(sim: &mut Simulation, id: u64) -> GameEntity {
        GameEntity::new_at_frame_zero_for_test(
            id,
            10,
            10,
            0,
            0,
            sim.interner.intern("Americans"),
            Health::new(100),
            sim.interner.intern("MTNK"),
            EntityCategory::Unit,
            0,
            5,
            true,
        )
    }

    #[test]
    fn live_radio_contacts_change_state_hash_per_mover() {
        let mut sim_a = Simulation::new();
        let mut sim_b = Simulation::new();
        let mut contacted = vehicle_entity(&mut sim_a, 1);
        let unrelated = vehicle_entity(&mut sim_a, 2);
        let contacted_b = vehicle_entity(&mut sim_b, 1);
        let unrelated_b = vehicle_entity(&mut sim_b, 2);

        contacted.mark_live_contact_with(100);
        sim_a.substrate.entities.insert(contacted);
        sim_a.substrate.entities.insert(unrelated);
        sim_b.substrate.entities.insert(contacted_b);
        sim_b.substrate.entities.insert(unrelated_b);

        assert_ne!(
            sim_a.state_hash(),
            sim_b.state_hash(),
            "per-mover live contacts must affect deterministic state hash",
        );
        assert!(
            !sim_a
                .substrate
                .entities
                .get(2)
                .unwrap()
                .has_live_contact_with(100)
        );
    }

    #[test]
    fn despawn_contact_cleanup_hash_matches_never_contacted_state() {
        let mut with_stale_contact = Simulation::new();
        let mut never_contacted = Simulation::new();

        let mut removed = vehicle_entity(&mut with_stale_contact, 1);
        let mut survivor = vehicle_entity(&mut with_stale_contact, 2);
        removed.mark_live_contact_with(2);
        survivor.mark_live_contact_with(1);
        with_stale_contact.substrate.entities.insert(removed);
        with_stale_contact.substrate.entities.insert(survivor);

        let removed_b = vehicle_entity(&mut never_contacted, 1);
        let survivor_b = vehicle_entity(&mut never_contacted, 2);
        never_contacted.substrate.entities.insert(removed_b);
        never_contacted.substrate.entities.insert(survivor_b);

        for id in [1, 2] {
            assert!(matches!(
                with_stale_contact.reveal(id),
                crate::sim::world::RevealOutcome::Revealed { .. }
            ));
            assert!(matches!(
                never_contacted.reveal(id),
                crate::sim::world::RevealOutcome::Revealed { .. }
            ));
        }

        with_stale_contact.despawn_entity(1);
        never_contacted.despawn_entity(1);

        assert_eq!(
            with_stale_contact.state_hash(),
            never_contacted.state_hash(),
            "cleanup should leave the same hash as a sim that never carried the stale contact",
        );
    }
}

#[cfg(test)]
mod building_anim_slot_hash_tests {
    #[test]
    fn retained_slot_identity_flag_and_runtime_change_hash() {
        let (mut sim, rules, id) = crate::sim::building_art::slot_test_fixture();
        let empty = sim.state_hash();
        let anim = sim
            .set_building_anim_slot(id, 3, false, false, 0, &rules)
            .unwrap();
        let present = sim.state_hash();
        assert_ne!(empty, present);
        sim.substrate
            .anims
            .get_mut(anim)
            .unwrap()
            .runtime_mut_for_test()
            .current_frame += 1;
        assert_ne!(present, sim.state_hash());
        let advanced = sim.state_hash();
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .building_art
            .set_damage_state_active_for_test(true);
        assert_ne!(advanced, sim.state_hash());
        let retained = sim.state_hash();
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .building_art
            .anim_slots_mut_for_test()
            .swap(3, 4);
        assert_ne!(retained, sim.state_hash());
    }
}

#[cfg(test)]
mod infantry_hash_tests {
    use super::Simulation;
    use crate::map::entities::EntityCategory;
    use crate::sim::animation::{Animation, SequenceKind};
    use crate::sim::combat::Health;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::infantry::InfantryRuntime;

    fn hash_entity(sim: &mut Simulation, category: EntityCategory) -> GameEntity {
        GameEntity::new_at_frame_zero_for_test(
            1,
            0,
            0,
            0,
            0,
            sim.interner.intern("Allies"),
            Health::new(100),
            sim.interner.intern("E1"),
            category,
            0,
            5,
            false,
        )
    }

    fn hash_with_animation(category: EntityCategory, animation: Option<Animation>) -> u64 {
        let mut sim = Simulation::new();
        let mut entity = hash_entity(&mut sim, category);
        entity.animation = animation;
        sim.substrate.entities.insert(entity);
        sim.state_hash()
    }

    #[test]
    fn every_gameplay_read_animation_field_changes_state_hash() {
        let hash = |animation| hash_with_animation(EntityCategory::Unit, animation);
        let absent = hash(None);
        let base = Animation::new(SequenceKind::Stand);
        let base_hash = hash(Some(base.clone()));
        assert_ne!(absent, base_hash);

        let mut sequence = base.clone();
        sequence.set_sequence_for_test(SequenceKind::Attack);
        assert_ne!(base_hash, hash(Some(sequence)));

        let mut frame_index = base.clone();
        frame_index.set_frame_index_for_test(1);
        assert_ne!(base_hash, hash(Some(frame_index)));

        let mut elapsed_frames = base.clone();
        elapsed_frames.set_elapsed_frames_for_test(1);
        assert_ne!(base_hash, hash(Some(elapsed_frames)));

        let mut finished = base;
        finished.set_finished_for_test(true);
        assert_ne!(base_hash, hash(Some(finished)));
    }

    #[test]
    fn infantry_presentation_animation_cannot_change_simulation_hash() {
        // The class owns Doing/Stage. A retained presentation component after
        // restore cannot become a second simulation clock through hashing.
        let absent = hash_with_animation(EntityCategory::Infantry, None);
        let mut animation = Animation::new(SequenceKind::Die1);
        animation.set_frame_index_for_test(u16::MAX);
        animation.set_elapsed_frames_for_test(u16::MAX);
        animation.set_finished_for_test(true);
        assert_eq!(
            absent,
            hash_with_animation(EntityCategory::Infantry, Some(animation))
        );
    }

    #[test]
    fn infantry_fear_prone_and_cell_entry_blocked_change_hash() {
        {
            let mut sim_a = Simulation::new();
            let mut sim_b = Simulation::new();
            let mut a = hash_entity(&mut sim_a, EntityCategory::Infantry);
            let b = hash_entity(&mut sim_b, EntityCategory::Infantry);
            a.infantry = Some({
                let mut runtime = InfantryRuntime::new();
                runtime.set_fear_level_for_test(10);
                runtime.set_is_prone_for_test(false);
                runtime
            });
            sim_a.substrate.entities.insert(a);
            sim_b.substrate.entities.insert(b);
            assert_ne!(sim_a.state_hash(), sim_b.state_hash());

            let mut sim_a = Simulation::new();
            let mut sim_b = Simulation::new();
            let mut a = hash_entity(&mut sim_a, EntityCategory::Infantry);
            let b = hash_entity(&mut sim_b, EntityCategory::Infantry);
            a.infantry = Some({
                let mut runtime = InfantryRuntime::new();
                runtime.set_fear_level_for_test(0);
                runtime.set_is_prone_for_test(true);
                runtime
            });
            sim_a.substrate.entities.insert(a);
            sim_b.substrate.entities.insert(b);
            assert_ne!(sim_a.state_hash(), sim_b.state_hash());
        }
        {
            let mut sim_a = Simulation::new();
            let mut sim_b = Simulation::new();
            let mut a = hash_entity(&mut sim_a, EntityCategory::Infantry);
            let b = hash_entity(&mut sim_b, EntityCategory::Infantry);
            a.infantry = Some({
                let mut runtime = InfantryRuntime::new();
                runtime.set_cell_entry_blocked_for_test(true);
                runtime
            });
            sim_a.substrate.entities.insert(a);
            sim_b.substrate.entities.insert(b);
            assert_ne!(sim_a.state_hash(), sim_b.state_hash());
            // A clear byte folds nothing: prior streams keep their pins.
            let mut sim_c = Simulation::new();
            let c = hash_entity(&mut sim_c, EntityCategory::Infantry);
            sim_c.substrate.entities.insert(c);
            assert_eq!(sim_b.state_hash(), sim_c.state_hash());
        }
    }

    #[test]
    fn foot_path_runtime_hash_and_snapshot_survive_without_movement_adapter() {
        use crate::sim::components::FootPathRuntime;
        use crate::sim::timer::CdTimer;
        let mut sim = Simulation::new();
        let actor = hash_entity(&mut sim, EntityCategory::Infantry);
        sim.substrate.entities.insert(actor);
        let before = sim.state_hash();
        let mut retained = FootPathRuntime::at_frame(0);
        retained.set_movement_timer_for_test(CdTimer::from_raw(-1, -7));
        retained.set_blocked_timer_for_test(CdTimer::from_raw(i32::MAX - 2, 31));
        retained.set_path_blocked_for_test(true);
        retained.set_retries_left_for_test(u32::MAX);
        sim.substrate
            .entities
            .get_mut(1)
            .unwrap()
            .navigation
            .set_path_runtime_for_test(retained);
        assert_ne!(sim.state_hash(), before);
        let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "foot-path", 0);
        let loaded = crate::sim::snapshot::GameSnapshot::load(&bytes)
            .unwrap()
            .sim;
        let actor = loaded.entities().get(1).unwrap();
        assert!(crate::sim::movement::motion_query::is_moving(&actor) != Some(true));
        assert_eq!(actor.navigation.path_runtime().clone(), retained);
    }

    #[test]
    fn foot_retarget_snapshot_retains_native_next_scan_mask_and_empty_clear() {
        let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/anytown_damage/foot_missions.json",
        ))
        .unwrap();
        let row = native["greatest_threat_rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["input"]["name"] == "MTNK_concrete_mask0_latch1_MTNK_live0")
            .unwrap();
        let input_mask = row["greatest_threat_masks"]["foot_greatest"][0]
            .as_u64()
            .unwrap() as u32;
        let effective_mask = row["greatest_threat_masks"]["greatest"][0]
            .as_u64()
            .unwrap() as u32;
        assert_eq!(row["before"]["scan"], 1);
        assert_eq!(row["returned_eax"], 0);
        assert_eq!(row["after"]["scan"], 0);

        let mut sim = Simulation::new();
        // Native Scenario load reseeds its stream to zero. Isolate this
        // retained Foot state from that established load-time RNG change.
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let actor = hash_entity(&mut sim, EntityCategory::Infantry);
        sim.substrate.entities.insert(actor);
        let clear_hash = sim.state_hash();
        sim.substrate
            .entities
            .get_mut(1)
            .unwrap()
            .foot_retarget_after_stop
            .mark_stopped_cannot_fire();
        let retained_hash = sim.state_hash();
        assert_ne!(retained_hash, clear_hash);
        let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "foot-retarget", 0);
        let mut loaded = crate::sim::snapshot::GameSnapshot::load(&bytes)
            .unwrap()
            .sim;
        assert_eq!(loaded.state_hash(), retained_hash);
        let actor = loaded.substrate.entities.get_mut(1).unwrap();
        assert!(actor.foot_retarget_after_stop());
        assert_eq!(
            actor.foot_retarget_after_stop.coerce_mask(input_mask),
            effective_mask
        );
        actor.foot_retarget_after_stop.finish_scan(false);
        assert!(!actor.foot_retarget_after_stop());
        assert_eq!(
            actor.foot_retarget_after_stop.coerce_mask(input_mask),
            input_mask
        );
        assert_eq!(loaded.state_hash(), clear_hash);
    }

    #[test]
    fn inherited_foot_firing_state_survives_snapshot_and_changes_noninfantry_hash() {
        let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/anytown_damage/foot_missions.json",
        ))
        .unwrap();
        let raw = native["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["input"]["name"] == "MTNK_area_guard_leash_firing_bypass")
            .unwrap()["before"]["firing"]
            .as_u64()
            .unwrap() as u8;
        // The Unit handler control supplies this byte. Aircraft also inherits
        // the raw persisted Foot field; its save/load assertion is a Rust
        // storage regression, not an Aircraft firing-producer parity claim.
        for (category, type_name) in [
            (EntityCategory::Unit, "MTNK"),
            (EntityCategory::Aircraft, "ORCA"),
        ] {
            let mut sim = Simulation::new();
            sim.scenario_rng = crate::sim::rng::SimRng::new(0);
            let owner = sim.interner.intern("Allies");
            let type_ref = sim.interner.intern(type_name);
            sim.substrate
                .entities
                .insert(GameEntity::new_at_frame_zero_for_test(
                    1,
                    0,
                    0,
                    0,
                    0,
                    owner,
                    Health::new(100),
                    type_ref,
                    category,
                    0,
                    5,
                    false,
                ));
            let clear_hash = sim.state_hash();
            sim.substrate
                .entities
                .get_mut(1)
                .unwrap()
                .mission_leaf
                .set_foot_firing_sequence(raw);
            let retained_hash = sim.state_hash();
            assert_ne!(retained_hash, clear_hash, "{type_name}");
            let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, type_name, 0);
            let mut loaded = crate::sim::snapshot::GameSnapshot::load(&bytes)
                .unwrap()
                .sim;
            assert_eq!(loaded.state_hash(), retained_hash, "{type_name}");
            assert_eq!(
                loaded
                    .entities()
                    .get(1)
                    .unwrap()
                    .mission_leaf
                    .foot_firing_sequence_latch(),
                raw,
                "{type_name}"
            );
            loaded
                .substrate
                .entities
                .get_mut(1)
                .unwrap()
                .mission_leaf
                .set_foot_firing_sequence(0);
            assert_eq!(loaded.state_hash(), clear_hash, "{type_name}");
        }
    }

    #[test]
    fn foot_scold_byte_survives_snapshot_and_changes_the_hash() {
        // Original raw Load and no-init Foot construction preserve 0, 1 and
        // 255 separately; the guard only distinguishes zero from nonzero.
        // Native comparison: tools/spatial_oracle/foot_scold_latch.json.
        let mut sim = Simulation::new();
        // Scenario deserialization deliberately reseeds to0. Start this
        // retained-byte fixture at that same state so its full-hash assertion
        // isolates persistence, not the intentionally changed RNG future.
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let actor = hash_entity(&mut sim, EntityCategory::Infantry);
        assert_eq!(actor.navigation.path_runtime().scold_latch_raw(), 0);
        sim.substrate.entities.insert(actor);
        let clear_hash = sim.state_hash();
        let mut retained_hashes = vec![clear_hash];
        let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/foot_scold_latch.json",
        ))
        .unwrap();
        for row in native["imported_latch"].as_array().unwrap() {
            let raw = row["supplied_saved_byte"].as_u64().unwrap() as u8;
            sim.substrate
                .entities
                .get_mut(1)
                .unwrap()
                .navigation
                .path_runtime_mut()
                .set_scold_latch_for_test(raw);
            let current_hash = sim.state_hash();
            assert!(!retained_hashes.contains(&current_hash) || raw == 0);
            retained_hashes.push(current_hash);

            let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "scold", 0);
            let mut restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
                .unwrap()
                .sim;
            let actor = restored.substrate.entities.get(1).unwrap();
            assert!(crate::sim::movement::motion_query::is_moving(&actor) != Some(true));
            assert_eq!(
                u64::from(actor.navigation.path_runtime().scold_latch_raw()),
                row["after_original_noinit_constructor"].as_u64().unwrap()
            );
            assert_eq!(restored.state_hash(), current_hash);
            let path = restored
                .substrate
                .entities
                .get_mut(1)
                .unwrap()
                .navigation
                .path_runtime_mut();
            assert_eq!(path.clear_scold_latch(), raw != 0);
            assert_eq!(path.scold_latch_raw(), 0);
            assert!(!path.clear_scold_latch(), "a consumed latch stays clear");
            assert_eq!(restored.state_hash(), clear_hash);
        }
    }
}

#[cfg(test)]
mod smudge_hash_tests {
    use super::*;
    use crate::sim::smudge_grid::{SmudgeCell, SmudgeGrid};

    #[test]
    fn hash_changes_when_smudge_placed() {
        let mut sim = Simulation::new();
        sim.smudge_grid = Some(SmudgeGrid::new(8, 8));
        let h0 = sim.state_hash();
        if let Some(grid) = sim.smudge_grid.as_mut() {
            grid.test_force_set(
                2,
                3,
                SmudgeCell {
                    type_id: Some(0),
                    footprint_origin: Some((2, 3)),
                    frame_offset: 0,
                },
            );
        }
        let h1 = sim.state_hash();
        assert_ne!(h0, h1);
    }
}

#[cfg(test)]
mod bridge_state_hash_tests {
    use super::Simulation;
    use crate::sim::bridge_state::{BridgeEndpointRecord, BridgeRecordKind, BridgeRuntimeState};

    #[test]
    fn bridge_endpoint_record_kind_difference_changes_state_hash() {
        let mut sim_a = Simulation::new();
        let mut sim_b = Simulation::new();

        let mut state_a = BridgeRuntimeState::default();
        let mut state_b = BridgeRuntimeState::default();
        let mut record = BridgeEndpointRecord {
            endpoint_a: (1, 1),
            endpoint_b: (4, 1),
            active: true,
            bridge_kind: BridgeRecordKind::High,
        };
        state_a.test_set_endpoint_records(vec![record]);
        record.bridge_kind = BridgeRecordKind::Low;
        state_b.test_set_endpoint_records(vec![record]);

        sim_a.bridge_state = Some(state_a);
        sim_b.bridge_state = Some(state_b);

        assert_ne!(
            sim_a.state_hash(),
            sim_b.state_hash(),
            "bridge endpoint record kind must contribute to state hash",
        );
    }

    #[test]
    fn one_advance_is_one_wrapping_native_frame_for_any_host_duration() {
        {
            let mut sim = Simulation::new();
            let host_durations = [0, 1, 22, 67, 1_000, u32::MAX];
            for (index, tick_ms) in host_durations.into_iter().enumerate() {
                sim.advance_tick(&[], None, None, None, tick_ms);
                assert_eq!(sim.session.binary_frame(), index as u32 + 1);
            }
        }
        {
            let mut sim = Simulation::new();
            sim.session.set_binary_frame_for_test(u32::MAX);

            sim.advance_tick(&[], None, None, None, 22);

            assert_eq!(sim.session.binary_frame(), 0);
        }
    }
}

#[cfg(test)]
mod native_frame_tests {
    use super::Simulation;

    #[test]
    fn native_frame_changes_state_hash() {
        let mut sim_a = Simulation::new();
        let sim_b = Simulation::new();
        sim_a.advance_tick(&[], None, None, None, 22);
        assert_ne!(sim_a.state_hash(), sim_b.state_hash());
    }

    #[test]
    fn diagnostic_total_sim_ms_does_not_change_state_hash() {
        let mut sim_a = Simulation::new();
        let sim_b = Simulation::new();
        sim_a.session.set_total_sim_ms_for_test(123_456);

        assert_eq!(sim_a.state_hash(), sim_b.state_hash());
    }
}

#[cfg(test)]
mod rocking_hash_tests {
    use super::Simulation;
    use crate::map::entities::EntityCategory;
    use crate::sim::combat::Health;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::rocking::RockingState;
    use crate::util::fixed_math::SimFixed;

    fn make_sim_with_one_vehicle() -> Simulation {
        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Americans");
        let type_id = sim.interner.intern("HTNK");
        let id = sim.next_stable_object_id();
        sim.set_next_stable_object_id_for_test(sim.next_stable_object_id() + 1);
        let e = GameEntity::new_at_frame_zero_for_test(
            id,
            10,
            10,
            0,
            0,
            owner,
            Health::new(400),
            type_id,
            EntityCategory::Unit,
            0,
            5,
            true,
        );
        sim.substrate.entities.insert(e);
        sim
    }

    #[test]
    fn rocking_none_and_angle_contribute_to_hash() {
        {
            let a = make_sim_with_one_vehicle();
            let b = make_sim_with_one_vehicle();
            assert_eq!(a.state_hash(), b.state_hash());

            // Mutate only the rocking state of one — hashes must diverge.
            let mut a = a;
            let id = a.substrate.entities.values().next().unwrap().stable_id();
            a.substrate.entities.get_mut(id).unwrap().rocking = Some({
                let mut value = RockingState::default();
                value.set_angle_sideways(SimFixed::lit("0.1"));
                value
            });
            assert_ne!(a.state_hash(), b.state_hash());
        }
        {
            let mut a = make_sim_with_one_vehicle();
            let b = make_sim_with_one_vehicle();
            let id = a.substrate.entities.values().next().unwrap().stable_id();
            a.substrate.entities.get_mut(id).unwrap().rocking = Some(RockingState::default());
            // a has Some(default), b has None — hashes must diverge.
            assert_ne!(a.state_hash(), b.state_hash());
        }
    }
}

#[cfg(test)]
mod c4_hash_tests {
    use super::Simulation;
    use crate::map::entities::EntityCategory;
    use crate::sim::c4_plant::PendingC4Detonation;
    use crate::sim::combat::Health;
    use crate::sim::game_entity::GameEntity;

    #[test]
    fn c4_state_changes_hash() {
        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Americans");
        let type_id = sim.interner.intern("GHOST");
        let id = sim.next_stable_object_id();
        sim.set_next_stable_object_id_for_test(sim.next_stable_object_id() + 1);
        let e = GameEntity::new_at_frame_zero_for_test(
            id,
            10,
            10,
            0,
            0,
            owner,
            Health::new(125),
            type_id,
            EntityCategory::Infantry,
            0,
            5,
            false,
        );
        sim.substrate.entities.insert(e);
        let h_initial = sim.state_hash();

        // Mutate pending_c4_detonation — hash must change.
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .pending_c4_detonation
            .set_for_test(Some(PendingC4Detonation::new(
                crate::sim::timer::CdTimer::started(100, 30),
                Some(7),
            )));
        let h_with_pending = sim.state_hash();
        assert_ne!(
            h_initial, h_with_pending,
            "pending_c4_detonation must affect state hash"
        );
    }
}

#[cfg(test)]
mod passenger_cargo_hash_tests {
    use super::Simulation;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::passenger::{PassengerCargo, PassengerRole};

    fn sim_with_sizes(first_size: f64, second_size: f64) -> Simulation {
        let mut sim = Simulation::new();
        let mut carrier = GameEntity::test_default(1, "BFRT", "Allied", 5, 5);
        let mut cargo = PassengerCargo::new(5, 0.0);
        cargo.board_forced_chain(&[(10, first_size)]);
        cargo.board_forced_chain(&[(11, second_size)]);
        carrier.passenger_role = PassengerRole::transport(cargo);
        sim.substrate.entities.insert(carrier);
        sim
    }

    #[test]
    fn passenger_cargo_size_mapping_and_owner_relation_change_hash() {
        {
            let a = sim_with_sizes(1.0, 3.0);
            let b = sim_with_sizes(3.0, 1.0);

            assert_ne!(a.state_hash(), b.state_hash());
        }
        {
            let mut sim = sim_with_sizes(1.0, 3.0);
            let outside = sim.state_hash();
            sim.substrate
                .entities
                .get_mut(1)
                .unwrap()
                .passenger_role
                .set_inside_transport(20, false);
            let inside = sim.state_hash();
            assert_ne!(inside, outside);
            sim.substrate
                .entities
                .get_mut(1)
                .unwrap()
                .passenger_role
                .set_inside_transport(21, false);
            assert_ne!(sim.state_hash(), inside);
            sim.substrate
                .entities
                .get_mut(1)
                .unwrap()
                .passenger_role
                .clear_carrier_relation();
            assert_eq!(sim.state_hash(), outside);
            assert_eq!(
                sim.substrate
                    .entities
                    .get(1)
                    .unwrap()
                    .passenger_role
                    .cargo()
                    .unwrap()
                    .passengers(),
                [11, 10]
            );
        }
    }
}

#[cfg(test)]
mod bridge161_hash_projection_tests {
    use super::*;
    use crate::map::entities::EntityCategory;
    use crate::rules::locomotor_type::LocomotorKind;
    use crate::sim::combat::Health;
    use crate::sim::components::DriveCoord;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::movement::locomotion::piggyback::LocomotorRuntimePayload;
    use crate::sim::movement::locomotor::LocomotorState;

    fn supplied_payload_world(kind: LocomotorKind, stashed: bool) -> Simulation {
        let mut sim = Simulation::new();
        let owner = sim.intern("Americans");
        let type_ref = sim.intern("HASHFOOT");
        let mut entity = GameEntity::new_at_frame_zero_for_test(
            1,
            5,
            5,
            0,
            0,
            owner,
            Health::new(100),
            type_ref,
            EntityCategory::Infantry,
            0,
            5,
            true,
        );
        let mut locomotor = LocomotorState::for_test_kind(kind);
        if let Some(fly) = locomotor.fly_runtime_mut() {
            let tenth = crate::util::native_x87::NativeF64Bits::from_bits(0x3fb9_9999_9999_999a);
            fly.set_target_speed_bits_for_test(tenth);
            fly.set_current_speed_bits_for_test(tenth);
        }
        if stashed {
            assert!(locomotor.begin_piggyback(LocomotorKind::Teleport, 0));
        }
        entity.locomotor = Some(locomotor);
        sim.substrate.entities.insert(entity);
        sim
    }

    fn mutate(payload: &mut LocomotorRuntimePayload, field: usize) {
        let coord = DriveCoord {
            x: 1664,
            y: 1408,
            z: 414,
        };
        match payload {
            LocomotorRuntimePayload::Walk(state) => match field {
                0 => state.set_head_for_test(Some(coord)),
                1 => state.set_destination_for_test(Some(coord)),
                2 => state.set_moving_for_test(true),
                3 => state.set_animation_moving_for_test(true),
                _ => unreachable!(),
            },
            LocomotorRuntimePayload::Hover(state) => state.set_head(Some(coord)),
            LocomotorRuntimePayload::Fly(state) => {
                // Adjacent binary64 values share the same old Q16 projection.
                let next = crate::util::native_x87::NativeF64Bits::from_bits(0x3fb9_9999_9999_999b);
                match field {
                    0 => state.set_target_speed_bits_for_test(next),
                    1 => state.set_current_speed_bits_for_test(next),
                    _ => unreachable!(),
                }
            }
            LocomotorRuntimePayload::Jumpjet(state) => {
                *state = match field {
                    0 => state.clone().with_destination_for_test(coord),
                    1 => state.clone().with_moving_for_test(true),
                    2 => state.clone().with_phase_for_test(2),
                    3 => {
                        let mut flight = state.flight();
                        flight.set_target_height_for_test(7);
                        state.clone().with_flight_for_test(flight)
                    }
                    4 => {
                        let mut flight = state.flight();
                        flight.set_current_speed_bits_for_test(1.0f64.to_bits());
                        state.clone().with_flight_for_test(flight)
                    }
                    5 => {
                        let mut params = state.params();
                        params.set_speed_for_test(99);
                        state.clone().with_params_for_test(params)
                    }
                    6 => state.clone().with_landing_latched_for_test(true),
                    _ => unreachable!(),
                };
            }
            _ => unreachable!("supplied bridge payload only"),
        }
    }

    #[test]
    fn bridge161_hashes_each_active_and_stashed_payload_field() {
        // This checks Rust hash composition over supplied retained state, not
        // a native checksum or a claim that these fixtures execute movement.
        for (kind, fields) in [
            (LocomotorKind::Walk, 4),
            (LocomotorKind::Hover, 1),
            (LocomotorKind::Fly, 2),
            (LocomotorKind::Jumpjet, 7),
        ] {
            for stashed in [false, true] {
                for field in 0..fields {
                    let before = supplied_payload_world(kind, stashed);
                    let mut after = supplied_payload_world(kind, stashed);
                    let loco = after
                        .substrate
                        .entities
                        .get_mut(1)
                        .unwrap()
                        .locomotor
                        .as_mut()
                        .unwrap();
                    let object = if stashed {
                        loco.piggyback_mut().unwrap().suspended_mut_for_test()
                    } else {
                        loco
                    };
                    mutate(object.runtime_payload_mut(), field);
                    assert_ne!(
                        before.state_hash(),
                        after.state_hash(),
                        "{kind:?} stashed={stashed} field={field}"
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod prism_support_hash_tests {}

#[cfg(test)]
#[path = "navigation_history_hash_tests.rs"]
mod navigation_history_hash_tests;
