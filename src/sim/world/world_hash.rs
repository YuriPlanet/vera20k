//! Deterministic state hashing for the Simulation.
//!
//! Produces a reproducible u64 hash over the entire simulation state:
//! tick counter, RNG state, production queues, fog-of-war, entity components.
//! Used for replay verification and desync detection in multiplayer.
//!
//! Dependency rules: same as sim/ (depends on rules/, map/; never render/ui/audio/net).

use std::hash::{Hash, Hasher};

use super::Simulation;

#[cfg(test)]
mod retained_cell_hash_tests {
    use super::*;
    use crate::map::resolved_terrain::ResolvedTerrainGrid;
    use crate::sim::combat::{AttackTarget, TargetKind};
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
        for retained_field in 0..7 {
            entity.attack_target = None;
            entity.suspended_attack_target = None;
            entity.set_archive_target(None);
            entity.navigation = Default::default();
            match retained_field {
                0 => {
                    entity.attack_target = Some(AttackTarget {
                        target: TargetKind::Cell(1, 2),
                    })
                }
                1 => entity.suspended_attack_target = Some(TargetKind::Cell(3, 4)),
                2 => entity.set_archive_target(Some(TargetKind::Cell(5, 6))),
                3 => entity.navigation.nav_com = Some(NavTargetRef::cell(7, 8)),
                4 => entity.navigation.nav_com_aux = Some(NavTargetRef::cell(9, 10)),
                5 => entity.navigation.suspended_nav_com = Some(NavTargetRef::cell(11, 12)),
                6 => entity.navigation.nav_queue.push(NavTargetRef::cell(13, 14)),
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
    house.ai_activation.production.hash(hasher);
    house.ai_activation.autocreate_allowed.hash(hasher);
    house.ai_activation.ai_triggers_active.hash(hasher);
    house.current_iq.hash(hasher);
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
        entity.category = EntityCategory::Unit;
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
    use crate::util::fixed_math::SimFixed;

    /// A Teleport unit, optionally on a Drive leg, with `mutate` applied to the
    /// active object or, on the leg, to the suspended Teleport.
    fn hash_with(stashed: bool, mutate: Option<fn(&mut LocomotorState)>) -> u64 {
        let mut sim = Simulation::new();
        let mut entity = GameEntity::test_default(1, "LOCO", "Americans", 2, 2);
        entity.category = EntityCategory::Unit;
        let mut locomotor = LocomotorState::for_test_kind(LocomotorKind::Teleport);
        if stashed {
            assert!(locomotor.begin_piggyback(LocomotorKind::Drive, 0));
        }
        if let Some(mutate) = mutate {
            match locomotor.piggyback.as_mut() {
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
        let mutations: [(&str, fn(&mut LocomotorState)); 7] = [
            ("kind", |l| l.kind = LocomotorKind::Walk),
            ("powered", |l| l.powered = false),
            ("layer", |l| l.layer = MovementLayer::Bridge),
            ("altitude", |l| l.altitude = SimFixed::from_num(5)),
            ("balloon_hover", |l| l.balloon_hover = true),
            ("speed_type", |l| l.speed_type = SpeedType::Wheel),
            ("movement_zone", |l| {
                l.movement_zone = MovementZone::Amphibious
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
    use crate::rules::ini_parser::IniFile;
    use crate::rules::ruleset::RuleSet;
    use crate::sim::particles::spark::SparkMotionStep;
    use crate::sim::particles::spark_world::{SparkCollisionWorld, slope_matrix};
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
        aircraft.airstrike_manager_present().hash(hasher);
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
    /// Deterministic state hash over canonicalized simulation state.
    ///
    /// Hashes clocks, Scenario RNG, production, fog, alliances, and all entity
    /// components in stable-entity-ID order (EntityStore keys_sorted) for determinism.
    pub fn state_hash(&self) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();

        self.session.tick.hash(&mut hasher);
        self.session.binary_frame.hash(&mut hasher);
        // ScenarioClass owns the sole saved/synchronized RNG. Main and MapGen
        // are process globals and are deliberately absent from this hash.
        self.scenario_rng.hash_state(&mut hasher);
        // Scenario+214 controls identities of future constructors and therefore
        // guided Bullet steering. The saved prefix/phase also controls a fresh
        // map-read continuation. This is independent of Rust stable handles.
        self.native_unique_ids.hash(&mut hasher);
        self.substrate.next_stable_object_id.hash(&mut hasher);
        self.substrate.next_air_tracker_order.hash(&mut hasher);
        // YR LogicClass trigger latches are save/lockstep state, even though
        // their camera/message outcomes stay app-owned and are not hashed.
        self.trigger_runtime.hash_state(&mut hasher);
        self.team_script_vm
            .hash_state(self.session.binary_frame as i32, &mut hasher);
        self.hash_playfield_authority(&mut hasher);

        // LogicClass active-object order — authoritative (drives reconciliation order).
        let order = self.substrate.logic.as_slice();
        order.len().hash(&mut hasher);
        for id in order {
            id.hash(&mut hasher);
        }
        self.substrate.display.fold_hash(&mut hasher);

        // PendingDeleteList is an independent ordered substrate fact. The
        // length delimiter distinguishes queue boundaries before the ordered
        // IDs are folded (duplicates are intentionally preserved here).
        self.substrate.pending_delete.len().hash(&mut hasher);
        for id in &self.substrate.pending_delete {
            id.hash(&mut hasher);
        }

        self.substrate.fold_raw_cell_occupation(&mut hasher);
        self.substrate.fold_hidden_occupation(&mut hasher);
        self.substrate.fold_air_slots(&mut hasher);
        self.substrate.fold_base_reservations(&mut hasher);
        self.substrate.occupancy.hash_memberships(&mut hasher);

        self.session.fold_game_options(&mut hasher);
        self.hash_houses(&mut hasher);
        self.hash_terminal_score_snapshot(&mut hasher);
        self.hash_production(&mut hasher);
        self.hash_power_states(&mut hasher);
        self.hash_fog_and_alliances(&mut hasher);
        self.hash_bridge_state(&mut hasher);
        // Native local graph updates retain route-selection history even
        // when the current terrain and House threat values are identical.
        // Ordinary native load rebuilds the hierarchy through581F50; its
        // resulting hash may intentionally differ from the live pre-save hash.
        if let Some(zones) = &self.zone_grid {
            b"retained-navigation-history-v1".hash(&mut hasher);
            zones.fold_navigation_history(&mut hasher);
        }
        // `resolved_terrain` is derived/skipped. Fold the exact saved real
        // CellClass `0x1180` values once through their serialized authority.
        // Historical pre-v28/pre-v29 provenance probes must omit both this
        // schema tag and the value authority introduced at snapshot v90.
        b"real-cell-bridge-flags-v2".hash(&mut hasher);
        self.real_cell_bridge_flags_0x1180.hash(&mut hasher);
        b"dynamic-terrain-cells-v1".hash(&mut hasher);
        self.dynamic_terrain_cells.hash(&mut hasher);
        self.hash_overlay_grid(&mut hasher);
        self.hash_crate_authority(&mut hasher);
        self.hash_smudge_grid(&mut hasher);
        self.hash_radiation(&mut hasher);
        self.kamikaze.fold_hash(&mut hasher);
        {
            self.hash_projectiles(&mut hasher);
            let shared_dummy_handle = self.effective_shared_cell_dummy();
            let shared_dummy = shared_dummy_handle.snapshot();
            let gap_flags = shared_dummy_handle.retained_bridge_flags() & 0xC00;
            let bridge_keeps_dummy = shared_dummy_handle.native_anchor()
                == Some(crate::map::cell_index::NativeCellIdentity::Dummy)
                || self.resolved_terrain.as_ref().is_some_and(|terrain| {
                    terrain.iter().any(|cell| {
                        cell.bridge_facts.native_anchor
                            == Some(crate::map::cell_index::NativeCellIdentity::Dummy)
                    })
                });
            {
                let extra_flags = shared_dummy_handle.raw_flags()
                    & !crate::map::bridge_facts::RETAINED_CELLCLASS_BRIDGE_FLAG_MASK;
                let anchor = shared_dummy_handle.native_anchor();
                if extra_flags != 0 || anchor.is_some() {
                    b"shared-cell-dummy-bridge-publication-v1".hash(&mut hasher);
                    extra_flags.hash(&mut hasher);
                    anchor.hash(&mut hasher);
                }
            }
            if gap_flags != 0 {
                b"shared-cell-dummy-gap-v1".hash(&mut hasher);
                gap_flags.hash(&mut hasher);
            }
            let shared_dummy_overlay = shared_dummy_handle.overlay_identity_state();
            b"shared-cell-dummy-land-v1".hash(&mut hasher);
            shared_dummy_handle.land_type().hash(&mut hasher);
            if shared_dummy_handle.neighbor_count() != 0 {
                b"shared-cell-dummy-neighbor-count-v1".hash(&mut hasher);
                shared_dummy_handle.neighbor_count().hash(&mut hasher);
            }
            let shared_dummy_tube = shared_dummy_handle.raw_tube_index();
            if shared_dummy_tube != -1 {
                b"shared-cell-dummy-tube-v1".hash(&mut hasher);
                shared_dummy_tube.hash(&mut hasher);
            }
            // Unlike the requested coordinate, native `+0x140 & 0x1180`
            // survives ordinary lookups and changes later bridge/FNPC/target
            // behavior even when no Bullet currently retains the dummy.
            b"shared-cell-dummy-spark-v4".hash(&mut hasher);
            shared_dummy.bridge_flags_0x1180.hash(&mut hasher);
            shared_dummy.level.hash(&mut hasher);
            shared_dummy.slope_type.hash(&mut hasher);
            // CellClass+0x44/+0x11E can be mutated through true-dummy wall
            // cleanup and later changes lookup-dependent wall behavior. Native
            // Resize reconstructs the process object, so this is synchronized
            // live-state authority rather than Scenario payload authority.
            b"shared-cell-dummy-overlay-v1".hash(&mut hasher);
            shared_dummy_overlay.hash(&mut hasher);
            if bridge_keeps_dummy
                || self.resolved_terrain.as_ref().is_some_and(|terrain| {
                    use crate::map::cell_index::NativeCellIdentity;
                    use crate::sim::combat::TargetKind;
                    use crate::sim::components::NavTargetRef;
                    let keeps_dummy = |target: TargetKind| {
                        target.cell_identity(terrain) == Some(NativeCellIdentity::Dummy)
                    };
                    self.substrate.entities.values().any(|entity| {
                        [
                            entity.attack_target.as_ref().map(|attack| attack.target),
                            entity.suspended_attack_target,
                            entity.archive_target(),
                        ]
                        .into_iter()
                        .flatten()
                        .any(keeps_dummy)
                            || [
                                entity.navigation.nav_com,
                                entity.navigation.nav_com_aux,
                                entity.navigation.suspended_nav_com,
                            ]
                            .into_iter()
                            .flatten()
                            .chain(entity.navigation.nav_queue.iter().copied())
                            .any(|target| match target {
                                NavTargetRef::Cell { rx, ry } => {
                                    keeps_dummy(TargetKind::Cell(rx, ry))
                                }
                                _ => false,
                            })
                    })
                })
                || self.projectiles.iter().any(|(_, projectile)| {
                    projectile.target == crate::sim::projectile::ProjectileTarget::DummyCell
                })
            {
                // Retained Cell pointers make the live coordinate affect
                // future range, pursuit and projectile behavior.
                b"shared-cell-dummy-target-v3".hash(&mut hasher);
                shared_dummy.coord.hash(&mut hasher);
            }
            self.hash_waves(&mut hasher);
        }
        self.hash_super_weapons(&mut hasher);
        self.hash_entities(&mut hasher);
        self.hash_anims(&mut hasher);
        self.hash_voxel_anims(&mut hasher);
        self.hash_particle_systems(&mut hasher);
        self.session.fold_identity(&mut hasher);
        self.session.pixel_conversion_bounds.hash(&mut hasher);

        hasher.finish()
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
        let Some(snapshot) = self.terminal_score_snapshot.as_ref() else {
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
        self.projectiles.len().hash(hasher);
        for (&id, projectile) in self.projectiles.iter() {
            id.hash(hasher);
            projectile.id.hash(hasher);
            projectile.native_unique_id.hash(hasher);
            projectile.source_id.hash(hasher);
            projectile.position.x.hash(hasher);
            projectile.position.y.hash(hasher);
            projectile.position.z.hash(hasher);
            projectile.launch_origin.hash(hasher);
            projectile.launch_target.hash(hasher);
            projectile.previous_cell.hash(hasher);
            hash_projectile_target(projectile.target, hasher);
            projectile.last_target_position.x.hash(hasher);
            projectile.last_target_position.y.hash(hasher);
            projectile.last_target_position.z.hash(hasher);
            projectile.payload.base_damage.hash(hasher);
            projectile.payload.warhead.index().hash(hasher);
            projectile.payload.weapon.index().hash(hasher);
            if projectile.payload.damage_multiplier()
                != crate::sim::projectile::ProjectilePayload::UNSCALED
            {
                b"prism-damage-multiplier-v1".hash(hasher);
                projectile.payload.damage_multiplier().hash(hasher);
            }

            projectile.speed_leptons_per_frame.hash(hasher);
            projectile.velocity.hash(hasher);
            projectile.trajectory.hash(hasher);
            projectile.guidance.hash(hasher);
            projectile.visual.hash(hasher);
            projectile.arm_timer.hash(hasher);
            projectile.fuse_frames_remaining.hash(hasher);
            projectile.ranged_fuse.hash(hasher);
            projectile.last_distance_half.hash(hasher);
            projectile.tracks_target.hash(hasher);
            projectile.target_expiry.hash(hasher);
            projectile.collision.level.hash(hasher);
            projectile.collision.subject_to_walls.hash(hasher);
            projectile.collision.native_cell_collision.hash(hasher);
            projectile.collision.dropping.hash(hasher);
            projectile.collision.subject_to_cliffs.hash(hasher);
            projectile.collision.flak_scatter.hash(hasher);
            projectile.collision.anti_air.hash(hasher);
            projectile.collision.airburst.hash(hasher);
            projectile.collision.inaccurate.hash(hasher);
            projectile.collision.floater.hash(hasher);
            projectile.collision.elasticity_bits.hash(hasher);
            projectile.on_bridge.hash(hasher);
            projectile.collision.arcing.hash(hasher);
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
        for (id, sys) in self.particle_systems().iter() {
            id.hash(hasher);
            sys.type_id.0.hash(hasher);
            sys.coords.x.hash(hasher);
            sys.coords.y.hash(hasher);
            sys.coords.z.hash(hasher);
            sys.lifetime.hash(hasher);
            sys.facing.hash(hasher);
            sys.done_spawning.hash(hasher);
            // The rest of the saved system state. `in_logic_vector` is not
            // saved (load rebuilds it from the logic vector), so it stays out.
            sys.offset.to_array().hash(hasher);
            sys.spawn_timer.to_bits().hash(hasher);
            sys.spark_spawn_frames.hash(hasher);
            sys.attached_entity.hash(hasher);
            sys.owner_entity.hash(hasher);
            sys.target_coords.to_array().hash(hasher);
            sys.owner_house.hash(hasher);
            sys.particles.len().hash(hasher);
            for p in &sys.particles {
                p.type_id.0.hash(hasher);
                p.coords.x.hash(hasher);
                p.coords.y.hash(hasher);
                p.coords.z.hash(hasher);
                p.lifetime_remaining.hash(hasher);
                p.animation_state.hash(hasher);
                p.translucency.hash(hasher);
                p.state_advance_counter.hash(hasher);
                p.marked_for_deletion.hash(hasher);
                p.origin.to_array().hash(hasher);
                p.direction.map(|v| v.to_bits()).hash(hasher);
                p.velocity.to_bits().hash(hasher);
                p.damage_counter.hash(hasher);
                p.state_ai_advance.hash(hasher);
                (p.drift_x, p.drift_y, p.drift_z).hash(hasher);
                p.prev_delta.map(|v| v.to_bits()).hash(hasher);
                match p.spark {
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
        }
    }

    /// The owner's stored buildings and other objects whose destruction is
    /// not yet recorded (test observation of the retired per-house counts).
    #[cfg(test)]
    pub(crate) fn owned_object_counts(&self, owner: crate::sim::intern::InternedId) -> (u32, u32) {
        self.substrate
            .entities
            .values()
            .filter(|entity| entity.owner() == owner && !entity.destruction_recorded)
            .fold((0, 0), |(buildings, others), entity| {
                if entity.category == crate::map::entities::EntityCategory::Structure {
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
            // Retained509400/481870 values affect future path/AI decisions.
            // The all-zero constructor state preserves historical hashes.
            let threat = house.spatial_threat_values();
            if threat.iter().any(|&value| value != 0) {
                b"house-spatial-threat-v1".hash(hasher);
                threat.hash(hasher);
            }
            house.economy.credits().hash(hasher);
            // The sole cash balance retains its original position in the hash stream.
            house.economy.spent_credits().hash(hasher);
            // The score feeds the terminal Scenario draw.
            house.economy.score().hash(hasher);
            // House504080/503040 preserves the kill, loss and built totals
            // through native load. The retained-ship chain needs both its
            // initial and terminal loss. Preserve zero streams.
            if house.stats != crate::sim::house_state::MatchStatistics::default() {
                b"house-match-statistics-v1".hash(hasher);
                house.stats.hash(hasher);
            }
            house.side_index.hash(hasher);
            house.is_human.hash(hasher);
            house.player_control.hash(hasher);
            (house.difficulty as i32).hash(hasher);
            house.rof_bias().bits().hash(hasher);
            house.is_defeated.hash(hasher);
            house.has_won.hash(hasher);
            house.has_lost.hash(hasher);
            house.outcome_state.hash(hasher);
            house.map_is_clear.hash(hasher);
            house.spy_sat_active.hash(hasher);
            house.tracking.hash_defeat_counters(hasher);
            // House+0x164/+0x168 (Tech Hospital / Machine Shop). Tagged so a
            // house that never owned one keeps the historical stream, as the
            // spatial-threat and match-statistics folds above do.
            if house.self_heal_infantry() != 0 || house.self_heal_units() != 0 {
                b"house-self-heal-v1".hash(hasher);
                house.self_heal_infantry().hash(hasher);
                house.self_heal_units().hash(hasher);
            }
            house.tech_level.hash(hasher);
            hash_house_ai_activation_fields(house, hasher);
            house.strategy_emergency.hash(hasher);
            house.grudge_scores.len().hash(hasher);
            for (other, score) in &house.grudge_scores {
                other.hash(hasher);
                score.hash(hasher);
            }
            house.enemy_house.hash(hasher);

            if let Some((rx, ry)) = house.base_center {
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
            house.alternate_base_center.hash(hasher);
            if !house.build_const_order.is_empty() {
                b"naval-build-const-house-v1".hash(hasher);
                house.build_const_order.len().hash(hasher);
                for stable_id in &house.build_const_order {
                    stable_id.hash(hasher);
                }
            }
            if !house.base_projection.buildings().is_empty() {
                b"house-buildings-v1".hash(hasher);
                house.base_projection.buildings().hash(hasher);
            }
            if !house.base_projection.factory_plants().is_empty() {
                b"house-factory-plants-v1".hash(hasher);
                house.base_projection.factory_plants().hash(hasher);
            }
            house.base_plan.percent_built.hash(hasher);
            house.base_plan.nodes.len().hash(hasher);
            for node in &house.base_plan.nodes {
                node.type_or_control.hash(hasher);
                node.packed_cell.hash(hasher);
                node.filled.hash(hasher);
                node.retry_count.hash(hasher);
            }
            house.base_plan_center.hash(hasher);
            house.base_reservation.hash(hasher);
            house.waypoint_edge.hash(hasher);
            // `HouseClass+0x242`, the sticky harvester no-ore latch — a
            // raw House byte in the native save block and lockstep CRC.
            house.harvester_no_ore.hash(hasher);
            // `HouseClass+0x57D4` funds-nag TimerStruct and the
            // `[0xA8F040]` low-power guard (`HouseClass::Update
            // 0x004F8B3C..0x004F8DAB`): both live in the native save.
            // Folded with a 64-bit start, as before the timer was a
            // `CdTimer`.
            i64::from(house.eva_funds_timer.start_frame()).hash(hasher);
            house.eva_funds_timer.duration().hash(hasher);
            house.eva_low_power_guard.hash(hasher);
            house.hash_event_notifications(hasher);
            house.hash_super_weapon_cells(hasher);
            house.repair_delay.to_bits().hash(hasher);
            house.repair_start_latch.hash(hasher);
            i64::from(house.repair_latch_timer.start_frame()).hash(hasher);
            house.repair_latch_timer.duration().hash(hasher);
            // Tagged and folded only off their constructor values, so a house
            // without computer production hashes as earlier schemas did.
            let gatherers = house.tracking.resource_gatherers();
            if house.ai_production != Default::default() || gatherers != 0 {
                b"ai-base-building-v1".hash(hasher);
                house.ai_production.hash(hasher);
                gatherers.hash(hasher);
            }
            let forces = house.tracking.force_values();
            if forces != Default::default() {
                b"house-force-values-v1".hash(hasher);
                forces.hash(hasher);
            }
            let docks = house.tracking.airport_docks();
            if docks != 0 {
                b"house-airport-docks-v1".hash(hasher);
                docks.hash(hasher);
            }
            let factories = house.tracking.factories();
            if factories != [0; 5] {
                b"house-factory-counters-v1".hash(hasher);
                factories.hash(hasher);
            }
            if house.strategy_timer != crate::sim::house_state::strategy_timer_at_construction() {
                b"house-strategy-timer-v1".hash(hasher);
                house.strategy_timer.hash(hasher);
            }
            house.team_creation.hash_state(hasher);
            if house.ai_unit_choices != Default::default() {
                b"house-ai-unit-choices-v1".hash(hasher);
                house.ai_unit_choices.hash(hasher);
            }
            house.tracking.hash_ai_team_counters(hasher);
        }
    }

    /// Hash all production-related state: queues, held objects, resources.
    fn hash_production(&self, hasher: &mut impl Hasher) {
        let retired_tiberium_fold = false;
        // The queue-of-record lives in the factory registry (active build = `Factory`
        // head fields; tail = `Factory.queue` of `QueueEntry`), and so does a completed
        // building waiting for placement; both fold in `hash_factory_registry`.
        // `remaining_base_frames` no longer exists (it was a `BuildQueueItem` field);
        // the sidebar ETA derives it from `progress` at view time and it is
        // intentionally NOT hashed (the 18->19 shape change).
        for (owner, categories) in self.production.primary_factory_entries() {
            owner.hash(hasher);
            for (category, sid) in categories {
                category.hash(hasher);
                sid.hash(hasher);
            }
        }
        self.production.next_enqueue_order.hash(hasher);
        self.hash_factory_registry(hasher); // P5b: the authoritative factory registry

        // Live ore/gem identity and quantity are folded by `hash_overlay_grid`.
        self.production
            .ore_growth_state
            .hash_state(hasher, retired_tiberium_fold);
        // Hash retained terrain animations, including TIBTRE ore generators.
        for (&(rx, ry), spawner) in &self.production.terrain_animations {
            rx.hash(hasher);
            ry.hash(hasher);
            spawner.hash(hasher);
        }
        for (&stable_id, terrain) in &self.production.terrain_objects {
            stable_id.hash(hasher);
            terrain.hash(hasher);
        }
        for (&(rx, ry), &stable_id) in &self.production.terrain_object_cells {
            rx.hash(hasher);
            ry.hash(hasher);
            stable_id.hash(hasher);
        }
        for &(rx, ry) in &self.production.tiberium_spawning_terrain_cells {
            rx.hash(hasher);
            ry.hash(hasher);
        }
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
        for (holder, f) in self.production.factories.holders_insertion_ordered() {
            // The building that holds a computer's factory (`BuildingClass+0x524`);
            // a House's factory folds as earlier schemas did.
            if let crate::sim::production::FactoryHolder::Building(building) = holder {
                b"building-factory-v1".hash(hasher);
                building.hash(hasher);
            }
            f.owner.hash(hasher);
            (f.category as u8).hash(hasher);
            f.insertion_seq.hash(hasher);
            f.progress.hash(hasher);
            f.step_rate_frames.hash(hasher);
            f.step_timer.start_frame().hash(hasher);
            f.step_timer.duration().hash(hasher);
            f.balance.hash(hasher);
            match &f.object {
                Some(o) => {
                    1u8.hash(hasher);
                    o.type_id.hash(hasher);
                    match o.entity_id {
                        Some(e) => {
                            1u8.hash(hasher);
                            e.hash(hasher);
                        }
                        None => 0u8.hash(hasher),
                    }
                    o.completion_accounted.hash(hasher);
                }
                None => 0u8.hash(hasher),
            }
            f.on_hold.hash(hasher);
            f.suspended.hash(hasher);
            f.manual.hash(hasher);
            if f.has_changed() {
                b"factory-changed-5d".hash(hasher);
            }
            // P5d: the queue-of-record (was the per-`BuildQueueItem` `queues_by_owner` fold,
            // now retired). Folds in FIFO (`VecDeque`) order — deterministic by construction.
            (f.queue.len() as u64).hash(hasher);
            for e in &f.queue {
                e.type_id.hash(hasher);
                e.enqueue_order.hash(hasher);
            }
        }
    }

    /// Hash per-player power states for deterministic replay.
    fn hash_power_states(&self, hasher: &mut impl Hasher) {
        // BTreeMap<InternedId, _> iterates in deterministic sorted order.
        for (owner_id, state) in &self.power_states {
            owner_id.hash(hasher);
            state.total_output.hash(hasher);
            state.total_drain.hash(hasher);
            state.hash_assessment_state(hasher);
            if state.has_drained_power_source {
                b"House.DrainedPowerSource".hash(hasher);
            }
        }
    }

    /// Hash fog-of-war visibility and house alliance data.
    fn hash_fog_and_alliances(&self, hasher: &mut impl Hasher) {
        self.fog.width.hash(hasher);
        self.fog.height.hash(hasher);
        for (owner, fog) in &self.fog.by_owner {
            owner.hash(hasher);
            fog.cells_raw().hash(hasher);
            fog.shroud_knowledge_raw().hash(hasher);
            // The CellClass ground bits the aircraft queries read.
            fog.ground_flags_raw().hash(hasher);
        }
        self.fog.gap_sources.hash(hasher);
        self.fog.sight_admissions.hash(hasher);
        self.fog.whole_map_revealed_owners.hash(hasher);
        b"fogged-object-footprints-v1".hash(hasher);
        self.fog.next_fogged_object_id.hash(hasher);
        self.fog.fogged_object_cells.hash(hasher);
        self.fog.fogged_objects.hash(hasher);
        self.fog.sensors_by_house.hash(hasher);
        // `CellClass+0xAC[house]` disguise-detect counters.
        // Behaviour-affecting like their `SensorsOfHouses` sibling above:
        // `FUN_004870F0` reads them inside `IsDisguisedTo`, which decides
        // target acquisition and so changes issued commands, not just
        // presentation.
        self.fog.disguise_detect_by_house.hash(hasher);
        self.fog.cloaked_by_houses.hash(hasher);
        for (owner, allies) in &self.house_alliances {
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
        for ry in 0..overlay_grid.height() {
            for rx in 0..overlay_grid.width() {
                let cell = overlay_grid.cell(rx, ry);
                rx.hash(hasher);
                ry.hash(hasher);
                cell.overlay_id.hash(hasher);
                cell.overlay_data.hash(hasher);
                cell.wall_owner.hash(hasher);
            }
        }
        b"retained-wall-neighbor-counts-v1".hash(hasher);
        // The presence tag the retired plane-less fixture mode folded as 0
        // keeps its place.
        1u8.hash(hasher);
        let counts = overlay_grid.retained_neighbor_counts();
        counts.len().hash(hasher);
        for count in counts {
            count.hash(hasher);
        }
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
        let mut entries: Vec<(u16, u16, Option<u16>, Option<(u16, u16)>, u8)> = grid
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
    /// products of deterministic IEEE ops, so the bits are lockstep-stable)
    /// and the site registry. Both maps iterate in sorted key order.
    fn hash_radiation(&self, hasher: &mut impl Hasher) {
        for (&(rx, ry), level) in self.radiation.iter_cells() {
            rx.hash(hasher);
            ry.hash(hasher);
            level.to_bits().hash(hasher);
        }
        for site in self.radiation.sites() {
            site.center.hash(hasher);
            site.spread.hash(hasher);
            site.level.hash(hasher);
            site.level_steps.hash(hasher);
            site.duration.hash(hasher);
            site.remaining.hash(hasher);
            site.level_timer.start_frame().hash(hasher);
            site.level_timer.duration().hash(hasher);
        }
    }

    /// Hash per-house superweapon state and active lightning storm.
    fn hash_super_weapons(&self, hasher: &mut impl Hasher) {
        for (owner, weapons) in &self.super_weapons {
            owner.hash(hasher);
            for (type_id, inst) in weapons {
                type_id.hash(hasher);
                inst.is_active.hash(hasher);
                inst.is_ready.hash(hasher);
                inst.is_suspended.hash(hasher);
                inst.charge_start_tick.hash(hasher);
                inst.charge_duration.hash(hasher);
                inst.charge_drain_state.hash(hasher);
                inst.ready_tick.hash(hasher);
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

    /// Hash all entity components in stable-entity-ID order.
    /// BTreeMap iterates in key order (= stable_id), so no manual sort needed.
    fn hash_entities(&self, hasher: &mut impl Hasher) {
        for entity in self.substrate.entities.values() {
            entity.stable_id().hash(hasher);
            if let Some(ammo) = entity.aircraft_ammo.as_ref() {
                b"aircraft-ammo-v170".hash(hasher);
                ammo.hash(hasher);
            }
            // GSI-09.01: the `BuildingClass+0x6D0/+0x6D8` ProduceCash
            // timer and the `TechnoClass+0x1CC/+0x1D0` drain link pair.
            // All three drive future wallet writes, so a divergence here
            // desyncs credits; every object carries them.
            entity.produce_cash_timer.hash(hasher);
            entity.drain_target.hash(hasher);
            entity.draining_me.hash(hasher);
            // ParasiteClass (Foot+69C) of dogs and drones, the victim's
            // Foot+694/+698 links, and Foot+6A0 once it has been armed.
            // `limbo_reselect` (+432) is deliberately absent: it is set
            // only for the local player's selection, so peers differ.
            if let Some(parasite) = entity.parasite.as_deref() {
                b"parasite-v193".hash(hasher);
                parasite.hash(hasher);
            }
            if entity.parasite_eating_me.is_some() || entity.parasite_launch_lock != 0 {
                b"parasite-victim-v193".hash(hasher);
                entity.parasite_eating_me.hash(hasher);
                entity.parasite_launch_lock.hash(hasher);
            }
            if entity.paralysis_timer.duration() != 0 {
                0x6a0_u32.hash(hasher);
                entity.paralysis_timer.hash(hasher);
            }
            if entity.has_been_captured {
                // Building+6E3: doubles the survivor divisor, widens the
                // survivor roll and skips the engineer roll at death.
                0x6e3_u32.hash(hasher);
            }
            if entity.techno_ctor_random_word != 0 || entity.structure_upgrade_link.is_some() {
                b"techno-constructor-v1".hash(hasher);
                entity.techno_ctor_random_word.hash(hasher);
                entity.structure_upgrade_link.hash(hasher);
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
            entity.native_unique_id.hash(hasher);
            if let Some(manager) = entity.slave_manager.as_ref() {
                b"slave-manager-v209".hash(hasher);
                manager.hash(hasher);
            }
            if entity.slave.owner().is_some() || !entity.slave.cargo().is_empty() {
                b"slave-v209".hash(hasher);
                entity.slave.owner().hash(hasher);
                entity.slave.cargo().len().hash(hasher);
                for bale in entity.slave.cargo() {
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
            entity.lifecycle.object_alive.hash(hasher);
            entity.lifecycle.in_limbo.hash(hasher);
            entity.lifecycle.cell_marked.hash(hasher);
            entity.dying.hash(hasher);
            entity.dirty_rect_eligible.hash(hasher);
            entity.destruction_recorded.hash(hasher);
            entity.infantry_terminal.hash(hasher);
            // TechnoClass+0x3D5 is mutable admission state, not a derived
            // position query: ordinary movement is promote-only while
            // teleport and Set_Clipped_LocalSize own exact demotions.
            entity.in_playfield.hash(hasher);
            entity.move_sound.is_active().hash(hasher);
            entity.crashing.hash(hasher);
            entity.crashing_seen.hash(hasher);
            if !entity.sinking.is_default() {
                b"techno-sinking-v1".hash(hasher);
                entity.sinking.hash(hasher);
            }
            entity.is_mission_only().hash(hasher);
            entity.move_sound.countdown().hash(hasher);
            entity.position.rx.hash(hasher);
            entity.position.ry.hash(hasher);
            entity.position.z.hash(hasher);
            // Most objects have no exact coordinate-Z override. Preserve their
            // pre-v62 hash stream while making TubeMovement's signed lepton Z
            // authoritative whenever it is present.
            if let Some(exact_z_leptons) = entity.position.exact_z_leptons {
                b"exact-object-z-leptons-v1".hash(hasher);
                exact_z_leptons.hash(hasher);
            }
            entity.position.sub_x.hash(hasher);
            entity.position.sub_y.hash(hasher);
            // The body FacingClass (`+0x388`), each Techno's one heading.
            entity.body_facing.hash(hasher);
            // The barrel elevation (`+0x370`), tagged, once an Unlimbo moved
            // it off its constructor value.
            if !entity.barrel_elevation_is_constructed() {
                b"barrel-elevation-v1".hash(hasher);
                entity.barrel_elevation().hash(hasher);
            }
            entity.body_frame_counter.hash(hasher);
            // Building+6E6 is retained transition state, independent of HP.
            if entity.building_damage_state_active {
                b"building-damage-state-v1".hash(hasher);
            }
            if entity.building_actually_placed
                || entity.building_anim_slots.iter().any(Option::is_some)
                || (entity.building_anim_effect_replay.iter().any(|v| *v))
            {
                b"building-anim-slots-v1".hash(hasher);
                entity.building_actually_placed.hash(hasher);
                entity.building_anim_slots.hash(hasher);
                entity.building_anim_effect_replay.hash(hasher);
            }
            if entity.building_storage != Default::default() {
                b"building-storage-v1".hash(hasher);
                entity.building_storage.hash(hasher);
            }
            if entity.building_last_operational {
                b"building-operational-v1".hash(hasher);
            }
            if !entity.building_stuff_enabled {
                b"building-stuff-disabled-v1".hash(hasher);
            }
            if entity.building_has_engineer {
                b"building-has-engineer-v1".hash(hasher);
            }
            // Infantry gameplay reads its class-owned Doing/Stage fields.
            // A restored presentation component is not a second state owner.
            if entity.category != crate::map::entities::EntityCategory::Infantry
                && let Some(animation) = entity.animation.as_ref()
            {
                b"entity-animation-v1".hash(hasher);
                animation.sequence.hash(hasher);
                animation.frame_index.hash(hasher);
                animation.elapsed_frames.hash(hasher);
                animation.finished.hash(hasher);
            }
            entity.owner().hash(hasher);
            entity.health.current.hash(hasher);
            entity.estimated_health.get().hash(hasher);
            entity.type_ref().hash(hasher);
            (entity.category as u8).hash(hasher);
            entity.foundation.hash(hasher);
            entity.building_hidden_occupancy.hash(hasher);
            entity.base_reservation_spacing.hash(hasher);
            entity.determines_waypoint_edge.hash(hasher);
            if entity.build_const_eligible {
                b"naval-build-const-entity-v1".hash(hasher);
                entity.build_const_eligible.hash(hasher);
            }
            entity.base_plan_type_index.hash(hasher);
            entity.base_plan_is_defense.hash(hasher);
            entity.base_plan_has_undeploy_target.hash(hasher);
            // The rank is sampled from the raw accumulator; its fold keeps its
            // place in the stream. The accumulator itself is folded too, so
            // two objects one kill apart inside the same rank are distinct.
            entity.veterancy().hash(hasher);
            entity.veterancy_raw.bits().hash(hasher);
            entity.veterancy_rank_cache.hash(hasher);
            // Folded only while armed so the legacy default-zero hash stream
            // is preserved (the `no_damage` pattern).
            if entity.elite_flash_frames != 0 {
                b"elite-flash-v1".hash(hasher);
                entity.elite_flash_frames.hash(hasher);
            }
            entity.armor_multiplier.bits().hash(hasher);
            // Folded only once a Firepower crate changed it, so the legacy
            // hash stream of every untouched object is preserved.
            if entity.firepower_multiplier != crate::util::native_x87::NativeF64Bits::ONE {
                b"firepower-mult-v1".hash(hasher);
                entity.firepower_multiplier.bits().hash(hasher);
            }
            entity.berserk.hash(hasher);
            entity.was_attacked_by_enemy.hash(hasher);
            if entity.category == crate::map::entities::EntityCategory::Structure {
                entity.ai_sellable.hash(hasher);
            }
            if entity.category == crate::map::entities::EntityCategory::Structure {
                entity.repairing.hash(hasher);
                entity.ai_repairable.hash(hasher);
            }
            if entity.ai_placement_timer != crate::sim::timer::CdTimer::default() {
                b"ai-placement-timer-v1".hash(hasher);
                entity.ai_placement_timer.hash(hasher);
            }
            b"base-defense-response-v1".hash(hasher);
            entity.base_defense_response.hash(hasher);
            entity.regular_crusher.hash(hasher);
            entity.drive_accelerates.hash(hasher);
            entity.damage_fire_state_active.hash(hasher);
            entity.damage_fire_anim_ids.hash(hasher);
            entity.vision_range.hash(hasher);
            entity.sight_is_zero.hash(hasher);
            entity.sight_refresh_timers.hash(hasher);
            if entity.gap_generator != crate::sim::vision::GapGeneratorRuntime::default() {
                b"gap-operational-v144".hash(hasher);
                entity.gap_generator.hash(hasher);
            }

            if let Some(ref movement) = entity.movement_target {
                1u8.hash(hasher);
                movement.speed.hash(hasher);
            } else {
                0u8.hash(hasher);
            }

            {
                // Preserve schema160's original four-field order. Deriving
                // the fold from the expanded struct would alter every prior
                // projection, including constructor-default Foot states.
                let path = &entity.navigation.path_runtime;
                path.movement_timer.hash(hasher);
                path.blocked_timer.hash(hasher);
                path.path_blocked.hash(hasher);
                path.retries_left.hash(hasher);
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
            entity.navigation.neighbor_state.hash(hasher);
            entity.navigation.path_replay.hash(hasher);
            entity.navigation.nav_com_aux.hash(hasher);
            entity.navigation.nav_com.hash(hasher);
            entity.navigation.suspended_nav_com.hash(hasher);
            entity.navigation.nav_queue.hash(hasher);
            entity.navigation.pending_arrival_clear.hash(hasher);

            entity.foot_speed.applied_fraction().hash(hasher);
            if entity.flight_attitude != Default::default() {
                0x2e8_u32.hash(hasher);
                entity.flight_attitude.hash(hasher);
            }
            entity.foot_speed.crate_multiplier().hash(hasher);
            entity.foot_occupation_enabled.hash(hasher);
            entity.foot_locomotor_swap_active.hash(hasher);
            // Techno+0x1F8 is up only between the Teleporter arm's can't-end
            // branch and the next Unit setter call; the tag keeps every
            // established stream unchanged while it is clear.
            if entity.setter_force_reassign {
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
                // the gate from state already hashed above (and from position,
                // facing and movement target, likewise hashed). Hashing a
                // derived predicate would pin a projection of the same state
                // twice, and it cannot diverge independently of its inputs.
            } else {
                0u8.hash(hasher);
            }

            entity.on_bridge.hash(hasher);
            entity
                .runtime_bridge_transition
                .pending_mismatch
                .hash(hasher);
            entity.spotlight_capable.hash(hasher);
            entity.building_light.hash(hasher);
            entity.low_bridge_tube_state.hash(hasher);
            if let Some(cloak) = entity.cloak.as_ref() {
                1u8.hash(hasher);
                cloak.state.hash(hasher);
                cloak.depth.hash(hasher);
                cloak.step_delta.hash(hasher);
                cloak.step_timer.timer.start_frame().hash(hasher);
                cloak.step_timer.speed.hash(hasher);
                cloak.step_timer.timer.duration().hash(hasher);
                cloak.recloak_delay.start_frame().hash(hasher);
                cloak.recloak_delay.duration().hash(hasher);
            } else {
                0u8.hash(hasher);
            }
            if let Some(deposit) = entity.sensor_deposit {
                1u8.hash(hasher);
                deposit.owner.hash(hasher);
                deposit.center.hash(hasher);
                deposit.add_radius.hash(hasher);
                deposit.remove_radius.hash(hasher);
                deposit.building_array.hash(hasher);
                // The cached `DetectDisguiseRange=` circle. It is the
                // radius Limbo will decrement, so it selects which
                // cells leave the disguise-detect plane.
                deposit.detect_disguise_radius.hash(hasher);
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

            if let Some(ref attack) = entity.attack_target {
                1u8.hash(hasher);

                attack.target.hash(hasher);
            } else {
                0u8.hash(hasher);
            }
            entity.rearm_timer.start_frame().hash(hasher);
            entity.rearm_timer.duration().hash(hasher);
            entity.weapon_burst.hash(hasher);
            entity.pending_building_fire.hash(hasher);
            if entity.prism_support_count != 0 {
                b"prism-support-count-v1".hash(hasher);
                entity.prism_support_count.hash(hasher);
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
            match entity.dock_entered_with {
                Some(sid) => {
                    1u8.hash(hasher);
                    sid.hash(hasher);
                }
                None => 0u8.hash(hasher),
            }

            entity.hash_capture_infantry_type(hasher);
            entity.c4_plant.hash(hasher);
            match entity.pending_c4_detonation {
                Some(pending) => {
                    true.hash(hasher);
                    pending
                        .timer
                        .remaining(self.session.binary_frame as i32)
                        .hash(hasher);
                    pending.source_entity_id.hash(hasher);
                }
                None => false.hash(hasher),
            }
            entity.bunker_occupant.hash(hasher);
            // Reciprocal link + install machine are authoritative lifecycle state.
            entity.bunker_link.hash(hasher);
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
            if entity.mcv_deploy_pending {
                0x4d435644u32.hash(hasher);
                entity.mcv_deploy_pending.hash(hasher);
            }
            entity.hash_deploy_attachment(hasher);

            if let Some(infantry) = entity.infantry {
                1u8.hash(hasher);
                infantry.fear_level.hash(hasher);
                infantry.is_prone.hash(hasher);
                // The idle-fidget countdown gates a scenario-RNG draw, so a
                // divergence here becomes a divergence of every later draw.
                // Folded in every schema variant rather than behind a gate: the
                // Scenario RNG itself is folded before either gate, and the two
                // provenance fixtures both hold infantry eligible on their first
                // tick, so their legacy probes already move with the draws this
                // timer schedules. Gating it would hide the field without
                // buying those probes back.
                infantry.idle_action_timer.hash(hasher);
                // Infantry+6DC is written only by the failed-path receiver
                // 0x51DAF0; the tagged extension keeps every established
                // stream unchanged until a receiver actually stores 1.
                if infantry.cell_entry_blocked {
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
                (miner.kind as u8).hash(hasher);
                (miner.cargo.len() as u16).hash(hasher);
                for bale in &miner.cargo {
                    (bale.resource_type as u8).hash(hasher);
                    bale.value.hash(hasher);
                }
                let retired_dock_fold = false;
                if retired_dock_fold {
                    // home_refinery: None.
                    None::<u64>.hash(hasher);
                }
                miner.reserved_refinery.hash(hasher);
                let native_ore_field = true;
                if !native_ore_field {
                    // The retired target_ore_cell (None) and harvest_timer
                    // (unarmed).
                    None::<(u16, u16)>.hash(hasher);
                    crate::sim::mission::MissionTimer::default().hash(hasher);
                }
                if native_ore_field {
                    // Unit+0x6D1/+0x6D2; the shared Stage is folded above.
                    miner.unload_active.hash(hasher);
                    miner.harvesting.hash(hasher);
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
            match &entity.passenger_role {
                crate::sim::passenger::PassengerRole::None => {
                    0u8.hash(hasher);
                }
                crate::sim::passenger::PassengerRole::Transport { cargo } => {
                    1u8.hash(hasher);
                    cargo.capacity.hash(hasher);
                    (cargo.passengers.len() as u32).hash(hasher);
                    for &pid in &cargo.passengers {
                        pid.hash(hasher);
                    }
                    debug_assert_eq!(cargo.passenger_sizes.len(), cargo.passengers.len());
                    for &passenger_size in &cargo.passenger_sizes {
                        passenger_size.hash(hasher);
                    }
                    cargo.total_size.hash(hasher);
                    // `UnitClass+0x6E4` keep-count, written only by a
                    // transport's own Unload state 0. Folded only when set so
                    // objects that never unloaded (every pinned harness
                    // object) keep their v-schema bytes.
                    if entity.transport_unload_keep_count != 0 {
                        entity.transport_unload_keep_count.hash(hasher);
                    }
                }
                crate::sim::passenger::PassengerRole::Boarding {
                    target_transport_id,
                } => {
                    2u8.hash(hasher);
                    target_transport_id.hash(hasher);
                }
                crate::sim::passenger::PassengerRole::Inside {
                    transport_id,
                    open_topped,
                } => {
                    3u8.hash(hasher);
                    transport_id.hash(hasher);
                    // `+0x82`, folded only when set so a closed transport's
                    // cargo keeps its historical hash.
                    if *open_topped {
                        1u8.hash(hasher);
                    }
                }
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
            if let Some(ref manager) = entity.spawn_manager {
                1u8.hash(hasher);
                manager.hash(hasher);
            }
            if let Some(owner_id) = entity.spawn_owner_id {
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

            // Body rocking state. I16F16 doesn't implement Hash directly;
            // .to_bits() gives the underlying i32. Drive/Ship slope state is
            // hashed with its active/stashed locomotor payload below.
            if let Some(ref r) = entity.rocking {
                1u8.hash(hasher);
                r.angle_sideways.to_bits().hash(hasher);
                r.angle_forwards.to_bits().hash(hasher);
                r.vel_sideways.to_bits().hash(hasher);
                r.vel_forwards.to_bits().hash(hasher);
                r.is_ship_rocking.hash(hasher);
            } else {
                0u8.hash(hasher);
            }

            hash_mission_com(&entity.mission, hasher);
            hash_mission_leaf(&entity.mission_leaf, hasher);
            entity.occupier.hash(hasher);
            entity.passive_scan_timer.hash(hasher);
            // Passive-acquire bookkeeping. `passively_acquired_target` gates
            // the stale-target drop and the off-mission clear, so a
            // divergence here changes future targets; the scan-frame stamp
            // rides along in the same block.
            entity.last_target_scan_frame.hash(hasher);
            entity.passively_acquired_target.hash(hasher);
            // Foot688 changes the next scan's topology/radius. Constructor
            // false preserves earlier streams; the retained true state is an
            // independent schema255 contribution, never inferred from TarCom.
            if entity.foot_retarget_after_stop() {
                0x004D_9920_u32.hash(hasher);
                true.hash(hasher);
            }
            match entity.suspended_attack_target {
                Some(target) => {
                    1u8.hash(hasher);
                    target.hash(hasher);
                }
                None => 0u8.hash(hasher),
            }
            // ObjectClass +0x8D IsFallingDown and its FallRate: absent hashes
            // the one zero byte the retired falling byte did. The fall's
            // height is the Location Z, hashed with the position.
            match entity.parachute_state.as_ref() {
                Some(fall) => {
                    1u8.hash(hasher);
                    fall.rate.hash(hasher);
                }
                None => 0u8.hash(hasher),
            }

            // S4b damage-Spark `+0x308`-equivalent live-system gate. Hashed because
            // it gates future scenario_rng draws (a divergence here desyncs the
            // stream). Zero for every entity in stock YR (the gate is Cyborg-only).
            entity.damage_particle_live_until.hash(hasher);
            // ReceiveDamage damage-Smoke `+0x310` identity. This gates later
            // spawn/RNG and remains set while a marked system drains.
            entity.damage_smoke_system_id.hash(hasher);
        }
    }

    /// Scheduler-owned ordinary animations in stable-ID order. Render caches and
    /// transient sound events are deliberately excluded.
    fn hash_anims(&self, hasher: &mut impl Hasher) {
        self.substrate.anims.iter().count().hash(hasher);
        for (id, anim) in self.substrate.anims.iter() {
            id.hash(hasher);

            anim.hash(hasher);
            anim.bounce.is_some().hash(hasher);
            if let Some(body) = &anim.bounce {
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
        // An empty store contributes NO bytes. Debris exists only between a
        // death and the moment the last piece expires, so hashing a length of
        // zero on every other frame would move every established golden and
        // every historical schema probe for a plumbing reason rather than a
        // behavioural one — and would keep doing so at each future store. It
        // masks nothing: the fold is domain-separated by
        // `b"voxel-anim-debris-v1"`, so an empty store carries no information a
        // `0usize` would add.
        //
        // Be aware when reading this that it does NOT match its neighbours.
        // `fold_raw_cell_occupation` is the precedent, but that is a sparse
        // GRID fold; the two STORE folds this one sits between — `hash_anims`
        // and `hash_particle_systems` — both write their length
        // unconditionally. The skip was chosen to avoid a re-baseline, and it
        // is load-bearing for that reason alone.
        //
        // Coverage gap: no parity harness and no replay fixture authors a
        // positive `MaxDebris=`, so nothing red today exercises either this
        // fold's populated arm or the producer that fills it. The first fixture
        // that kills a `MaxDebris>0` type moves the goldens through the RNG
        // cursor even before a single piece survives to be folded.
        if self.substrate.voxel_anims.is_empty() {
            return;
        }
        b"voxel-anim-debris-v1".hash(hasher);
        self.substrate.voxel_anims.len().hash(hasher);
        for (id, debris) in self.substrate.voxel_anims.iter() {
            id.hash(hasher);
            debris.type_id.0.hash(hasher);
            debris.duration.hash(hasher);
            debris.marked_for_deletion.hash(hasher);
            debris.owner_house.hash(hasher);
            debris.bounce.hash_bits(hasher);
        }
    }
}

/// One locomotor object, field for field, and the object its piggyback slot
/// suspends through the same fold. The destructuring names every field, and
/// `deny(unused_variables)` makes a named but unfolded field a compile error;
/// `locomotor_field_hash_tests` checks that each one reaches the hash.
#[deny(unused_variables)]
fn hash_locomotor(
    loco: &crate::sim::movement::locomotor::LocomotorState,
    hasher: &mut impl Hasher,
) {
    let crate::sim::movement::locomotor::LocomotorState {
        kind,
        powered,
        piggyback,
        runtime_payload,
        layer,
        altitude,
        balloon_hover,
        speed_type,
        movement_zone,
    } = loco;
    // The installed class is the kind of the bottom object, which this
    // fold reaches through the stash.
    (*kind as u8).hash(hasher);
    powered.hash(hasher);
    (*layer as u8).hash(hasher);
    // SimFixed has no Hash; fold the raw bits.
    altitude.to_bits().hash(hasher);
    balloon_hover.hash(hasher);
    speed_type.hash(hasher);
    movement_zone.hash(hasher);
    hash_locomotor_payload(runtime_payload, hasher);
    match piggyback.as_deref() {
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
            state.target_speed.to_bits().hash(hasher);
            state.current_speed.to_bits().hash(hasher);
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
mod teleport_hash_tests {
    use super::Simulation;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::movement::teleport_movement::{TeleportPhase, TeleportState};

    fn teleport_state() -> TeleportState {
        TeleportState::for_test(TeleportPhase::Relocate, 17, 29, 41)
    }

    fn hash_entity(mut entity: GameEntity) -> u64 {
        let mut sim = Simulation::new();
        entity.stable_id = 1;
        sim.substrate.entities.insert(entity);
        sim.state_hash()
    }

    fn hash_teleport(state: Option<TeleportState>) -> u64 {
        let mut entity = GameEntity::test_default(1, "CHRP", "Americans", 5, 5);
        entity.install_teleport_state_for_test(state);
        hash_entity(entity)
    }

    fn assert_teleport_change(change: impl FnOnce(&mut TeleportState)) {
        let baseline = hash_teleport(Some(teleport_state()));
        let mut changed = teleport_state();
        change(&mut changed);
        assert_ne!(baseline, hash_teleport(Some(changed)));
    }

    #[test]
    fn teleport_hash_projects_presence_phase_target_and_materialization_timer() {
        assert_ne!(hash_teleport(None), hash_teleport(Some(teleport_state())));
        assert_teleport_change(|state| state.set_phase_for_test(TeleportPhase::ChronoDelay));
        assert_teleport_change(|state| {
            let mut destination = state.destination().unwrap();
            destination.x += 256;
            state.set_destination_for_test(destination);
        });
        assert_teleport_change(|state| {
            let mut destination = state.destination().unwrap();
            destination.y += 256;
            state.set_destination_for_test(destination);
        });
        assert_teleport_change(|state| state.set_ticks_for_test(state.being_warped_ticks() + 1));
    }
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

    #[test]
    fn lifecycle_authority_each_axis_changes_state_hash() {
        assert_entity_mutation_changes_hash(|entity| {
            entity.lifecycle.object_alive = !entity.lifecycle.object_alive;
        });
        assert_entity_mutation_changes_hash(|entity| {
            entity.lifecycle.in_limbo = !entity.lifecycle.in_limbo;
        });
        assert_entity_mutation_changes_hash(|entity| {
            entity.lifecycle.cell_marked = !entity.lifecycle.cell_marked;
        });
        assert_entity_mutation_changes_hash(|entity| {
            entity.dying = !entity.dying;
        });
        assert_entity_mutation_changes_hash(|entity| {
            entity.dirty_rect_eligible = !entity.dirty_rect_eligible;
        });
        assert_entity_mutation_changes_hash(|entity| {
            entity.destruction_recorded = !entity.destruction_recorded;
        });
        assert_entity_mutation_changes_hash(|entity| {
            entity.occupier = !entity.occupier;
        });
        assert_entity_mutation_changes_hash(|entity| {
            entity.in_playfield = !entity.in_playfield;
        });
        assert_entity_mutation_changes_hash(|entity| {
            entity.passive_scan_timer.arm(7, 12);
        });
    }

    #[test]
    fn lifecycle_authority_pending_queue_order_and_length_change_state_hash() {
        let mut empty = Simulation::new();
        let empty_hash = empty.state_hash();

        empty.substrate.pending_delete.push(1);
        let one_hash = empty.state_hash();
        empty.substrate.pending_delete.push(2);
        let ordered_hash = empty.state_hash();
        empty.substrate.pending_delete.swap(0, 1);
        let reversed_hash = empty.state_hash();

        assert_ne!(empty_hash, one_hash);
        assert_ne!(one_hash, ordered_hash);
        assert_ne!(ordered_hash, reversed_hash);
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
        entity.suspended_attack_target = target;
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
    fn every_unit_mission_leaf_field_changes_state_hash() {
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

    #[test]
    fn every_infantry_mission_leaf_field_changes_state_hash() {
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

    #[test]
    fn every_aircraft_mission_leaf_field_changes_state_hash() {
        let base = MissionLeafState::aircraft_raw_for_test(9, 10, false);
        let base_hash = hash_leaf(base);
        for (field, variant) in [
            (
                "action latch",
                MissionLeafState::aircraft_raw_for_test(10, 10, false),
            ),
            (
                "transition ready latch",
                MissionLeafState::aircraft_raw_for_test(9, 11, false),
            ),
            (
                "airstrike manager presence",
                MissionLeafState::aircraft_raw_for_test(9, 10, true),
            ),
        ] {
            assert_ne!(
                base_hash,
                hash_leaf(variant),
                "Aircraft {field} must contribute to the state hash"
            );
        }
    }

    #[test]
    fn building_mission_leaf_ready_latch_changes_state_hash() {
        assert_ne!(
            hash_leaf(MissionLeafState::building_raw_for_test(0)),
            hash_leaf(MissionLeafState::building_raw_for_test(1)),
            "Building ready latch must contribute to the state hash"
        );
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
        faster.parachute_state.as_mut().unwrap().rate = -2;

        let falling_hash = hash_entity(falling);
        assert_ne!(hash_entity(base), falling_hash, "IsFallingDown");
        assert_ne!(falling_hash, hash_entity(faster), "fall rate");
    }
}

#[cfg(test)]
mod track_authority_hash_tests {
    use super::Simulation;
    use crate::rules::locomotor_type::LocomotorKind;
    use crate::sim::components::{DriveCoord, DriveOccupationFootprint, TrackProgress};
    use crate::sim::game_entity::GameEntity;
    use crate::sim::movement::locomotor::LocomotorState;
    use crate::sim::movement::locomotor::MovementLayer;
    use crate::sim::movement::track_process::TrackFamily;
    use crate::sim::movement::{DriveLocomotionRuntime, ShipLocomotionRuntime};

    fn supplied_track_world(ship: bool, active: bool) -> Simulation {
        let mut sim = Simulation::new();
        let mut entity = GameEntity::test_default(1, "MTNK", "Americans", 10, 10);
        let track = if active {
            TrackProgress {
                turn_index: 0,
                cursor: 0,
                ..Default::default()
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
        let family = TrackFamily::from_kind(loco.kind).unwrap();
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
                        0 => track.turn_index = 1,
                        1 => track.cursor = 1,
                        2 => track.reversed = true,
                        3 => track.residual = 1,
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
    fn current_hash_covers_retained_coordinates_speed_permission_and_occupation() {
        let point = DriveCoord::cell(3, 4, 731);
        let mark = DriveOccupationFootprint {
            rx: 3,
            ry: 4,
            layer: MovementLayer::Bridge,
        };
        let drives = [
            DriveLocomotionRuntime::default().with_destination_for_test(Some(point)),
            DriveLocomotionRuntime::default().with_head_to_for_test(Some(point)),
            DriveLocomotionRuntime::default()
                .with_target_speed_fraction_for_test(crate::util::fixed_math::SIM_HALF),
            DriveLocomotionRuntime::default().with_end_permitted_for_test(false),
            DriveLocomotionRuntime::default().with_occupation_head_to_for_test(Some(mark)),
            DriveLocomotionRuntime::default().with_occupation_handoff_for_test(Some(mark)),
        ];
        let ships = [
            ShipLocomotionRuntime::default().with_destination_for_test(Some(point)),
            ShipLocomotionRuntime::default().with_head_to_for_test(Some(point)),
            ShipLocomotionRuntime::default()
                .with_target_speed_fraction_for_test(crate::util::fixed_math::SIM_HALF),
            ShipLocomotionRuntime::default().with_occupation_head_to_for_test(Some(mark)),
            ShipLocomotionRuntime::default().with_occupation_handoff_for_test(Some(mark)),
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
    use crate::rules::locomotor_type::LocomotorKind;
    use crate::sim::components::DriveCoord;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::movement::locomotor::LocomotorState;
    use crate::sim::movement::track_process::TrackFamily;

    /// A factory's rally point is its ArchiveTarget cell, folded with the
    /// base-defence state.
    #[test]
    fn factory_rally_point_changes_state_hash() {
        let factory = GameEntity::test_default(1, "GAWEAP", "Americans", 10, 10);
        let mut rallied = factory.clone();
        rallied.set_archive_target(Some(crate::sim::combat::TargetKind::Cell(30, 31)));
        let mut sim_a = Simulation::new();
        let mut sim_b = Simulation::new();
        sim_a.substrate.entities.insert(factory);
        sim_b.substrate.entities.insert(rallied);
        assert_ne!(sim_a.state_hash(), sim_b.state_hash());
    }

    #[test]
    fn drive_locomotion_state_changes_state_hash() {
        let mut sim_a = Simulation::new();
        let mut sim_b = Simulation::new();
        let mut entity_a = GameEntity::test_default(1, "AMCV", "Americans", 10, 10);
        entity_a.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
        let mut entity_b = entity_a.clone();
        entity_b.navigation.path_replay.directions = vec![2, 2, 2, 2, 2];
        let loco = entity_b.locomotor.as_mut().unwrap();
        loco.ensure_installed_track_state();
        loco.store_track_destination(TrackFamily::Drive, Some(DriveCoord::cell(45, 40, 0)));
        let mut progress = loco.track_progress(TrackFamily::Drive).unwrap();
        progress.residual = 3;
        loco.store_track_progress(TrackFamily::Drive, progress);
        sim_a.substrate.entities.insert(entity_a);
        sim_b.substrate.entities.insert(entity_b);

        assert_ne!(sim_a.state_hash(), sim_b.state_hash());
    }

    #[test]
    fn drive_accelerates_changes_state_hash() {
        let mut sim_a = Simulation::new();
        let mut sim_b = Simulation::new();
        let entity_a = GameEntity::test_default(1, "GTNK", "Americans", 10, 10);
        let mut entity_b = entity_a.clone();
        entity_b.drive_accelerates = false;
        sim_a.substrate.entities.insert(entity_a);
        sim_b.substrate.entities.insert(entity_b);

        assert_ne!(sim_a.state_hash(), sim_b.state_hash());
    }

    #[test]
    fn house_difficulty_changes_state_hash() {
        use crate::sim::house_state::{HouseDifficulty, HouseState};

        let mut sim_a = Simulation::new();
        let mut sim_b = Simulation::new();
        let owner_a = sim_a.interner.intern("Computer1");
        let owner_b = sim_b.interner.intern("Computer1");
        assert_eq!(owner_a, owner_b);
        sim_a
            .houses
            .insert(owner_a, HouseState::new(owner_a, 0, None, false, 0, 10));
        let mut hard_house = HouseState::new(owner_b, 0, None, false, 0, 10);
        hard_house.difficulty = HouseDifficulty::Hard;
        sim_b.houses.insert(owner_b, hard_house);

        assert_ne!(sim_a.state_hash(), sim_b.state_hash());
    }

    #[test]
    fn house_rof_bias_changes_state_hash_at_its_difficulty() {
        use crate::sim::house_state::{HouseDifficulty, HouseState};

        // Same stored difficulty, different bias: only the game-mode arm of
        // `set_difficulty` folds the country's ROF into `HouseClass+0x1A8`.
        let hashes: Vec<u64> = [false, true]
            .into_iter()
            .map(|game_mode_nonzero| {
                let mut sim = Simulation::new();
                let owner = sim.interner.intern("Computer1");
                let mut house = HouseState::new(owner, 0, None, false, 0, 10);
                let general = crate::rules::ruleset::GeneralRules {
                    difficulty_rof: [0.8, 1.0, 1.2],
                    difficulty_repair_delay: [0.02; 3],
                    ..Default::default()
                };
                house.set_difficulty(
                    HouseDifficulty::Hard,
                    &general,
                    0.9,
                    game_mode_nonzero,
                    0,
                    0,
                );
                sim.houses.insert(owner, house);
                sim.state_hash()
            })
            .collect();
        assert_ne!(hashes[0], hashes[1]);
    }

    #[test]
    fn alternate_base_center_changes_state_hash_without_changing_primary_center() {
        use crate::sim::house_state::HouseState;

        let mut baseline = Simulation::new();
        let mut changed = Simulation::new();
        let owner = baseline.interner.intern("Computer1");
        let changed_owner = changed.interner.intern("Computer1");
        assert_eq!(owner, changed_owner);
        let mut house = HouseState::new(owner, 0, None, false, 0, 10);
        house.base_center = Some((41, 52));
        let mut changed_house = house.clone();
        changed_house.alternate_base_center = (93, 106);
        baseline.houses.insert(owner, house);
        changed.houses.insert(changed_owner, changed_house);

        assert_ne!(baseline.state_hash(), changed.state_hash());
        assert_eq!(
            baseline.houses[&owner].base_center,
            changed.houses[&owner].base_center
        );
    }

    #[test]
    fn gsi_04_05_base_reservation_state_changes_world_hash() {
        use crate::sim::house_state::HouseState;

        let mut sim_a = Simulation::new();
        let mut sim_b = Simulation::new();
        let owner_a = sim_a.interner.intern("Computer1");
        let owner_b = sim_b.interner.intern("Computer1");
        assert_eq!(owner_a, owner_b);
        sim_a
            .houses
            .insert(owner_a, HouseState::new(owner_a, 0, None, false, 0, 10));
        let mut changed = HouseState::new(owner_b, 0, None, false, 0, 10);
        changed.base_reservation.update_bounds(3, 4, 5, 6);
        changed
            .base_reservation
            .append_perimeter_cell_if_absent(u32::from(3u16) | (u32::from(4u16) << 16));
        sim_b.houses.insert(owner_b, changed);

        assert_ne!(sim_a.state_hash(), sim_b.state_hash());
    }

    #[test]
    fn gsi_04_05_base_plan_state_and_entity_facts_are_current_schema_hash_authority() {
        use crate::sim::base_plan::{BasePlanNode, pack_base_plan_cell};
        use crate::sim::house_state::HouseState;

        fn house_sim(nodes: Vec<BasePlanNode>, percent_built: i32) -> Simulation {
            let mut sim = Simulation::new();
            let owner = sim.interner.intern("Computer1");
            let mut house = HouseState::new(owner, 0, None, false, 0, 10);
            house.base_plan.percent_built = percent_built;
            house.base_plan.nodes = nodes;
            sim.houses.insert(owner, house);
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
        a.base_plan_type_index = 2;
        a.base_plan_is_defense = true;
        a.base_plan_has_undeploy_target = true;
        b.base_plan_type_index = 3;
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
            house.ai_activation = latches;
            sim.houses.insert(owner, house);
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
    fn gsi_04_05_house_strategy_emergency_fields_each_change_world_hash() {
        use crate::sim::house_state::HouseState;

        fn fixture() -> (Simulation, crate::sim::intern::InternedId) {
            let mut sim = Simulation::new();
            let owner = sim.interner.intern("Computer1");
            sim.houses
                .insert(owner, HouseState::new(owner, 0, None, false, 0, 10));
            (sim, owner)
        }

        let (baseline, _) = fixture();
        let baseline_hash = baseline.state_hash();

        let (mut mode, owner) = fixture();
        mode.houses.get_mut(&owner).unwrap().strategy_emergency.mode = 4;
        assert_ne!(baseline_hash, mode.state_hash(), "mode is hashed");

        let (mut bias, owner) = fixture();
        bias.houses
            .get_mut(&owner)
            .unwrap()
            .strategy_emergency
            .all_to_hunt_bias = true;
        assert_ne!(baseline_hash, bias.state_hash(), "bias latch is hashed");

        let (mut attack_frame, owner) = fixture();
        attack_frame
            .houses
            .get_mut(&owner)
            .unwrap()
            .strategy_emergency
            .last_building_attack_frame = -17;
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
            .strategy_emergency
            .last_attacker_house_index = 2;
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
            .strategy_timer
            .start(12, 106);
        assert_ne!(
            baseline_hash,
            timer.state_hash(),
            "Strategy timer is hashed"
        );
    }

    #[test]
    fn gsi_04_05_techno_base_defense_state_changes_world_hash() {
        let mut baseline = Simulation::new();
        let entity = crate::sim::game_entity::GameEntity::test_default(1, "E1", "Computer1", 3, 4);
        baseline.substrate.entities.insert(entity.clone());
        let baseline_hash = baseline.state_hash();

        let mut changed = Simulation::new();
        let mut entity = entity;
        entity.base_defense_response.recruitable_b = false;
        entity.set_archive_target(Some(crate::sim::combat::TargetKind::Entity(7)));
        entity.base_defense_response.cooldown = crate::sim::timer::CdTimer::started(12, 225);
        changed.substrate.entities.insert(entity);
        assert_ne!(baseline_hash, changed.state_hash());
    }

    #[test]
    fn gsi_04_16_waypoint_edge_is_lockstep_hash_authority() {
        use crate::sim::house_state::HouseState;

        let mut north = Simulation::new();
        let mut south = Simulation::new();
        let north_owner = north.interner.intern("Player");
        let south_owner = south.interner.intern("Player");
        assert_eq!(north_owner, south_owner);

        let mut north_house = HouseState::new(north_owner, 0, None, true, 0, 10);
        north_house.waypoint_edge = 0;
        north.houses.insert(north_owner, north_house);

        let mut south_house = HouseState::new(south_owner, 0, None, true, 0, 10);
        south_house.waypoint_edge = 2;
        south.houses.insert(south_owner, south_house);

        assert_ne!(north.state_hash(), south.state_hash());
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
        ParticleSystem {
            stable_id: 0,
            in_logic_vector: false,
            type_id: ParticleSystemTypeId(0),
            coords,
            offset: IVec3::ZERO,
            particles: Vec::new(),
            spawn_timer: SimFixed::from_num(0),
            lifetime: -1,
            spark_spawn_frames: 0,
            facing: 0x1D,
            attached_entity: None,
            owner_entity: None,
            target_coords: IVec3::ZERO,
            owner_house: None,
            done_spawning: false,
        }
    }

    fn insert_system(sim: &mut Simulation, mut system: ParticleSystem) -> u64 {
        let id = sim.allocate_stable_id();
        system.stable_id = id;
        sim.particle_systems_mut().insert(system);
        sim.reveal_particle_system(id, None);
        id
    }

    #[test]
    fn empty_particle_store_hashes_consistently() {
        let a = Simulation::new();
        let b = Simulation::new();
        assert_eq!(a.state_hash(), b.state_hash());
    }

    #[test]
    fn particle_state_changes_hash() {
        let mut sim = Simulation::new();
        let h1 = sim.state_hash();
        insert_system(&mut sim, fake_system(IVec3::new(100, 0, 0)));
        let h2 = sim.state_hash();
        assert_ne!(h1, h2);
    }

    #[test]
    fn state_advance_counter_changes_hash() {
        let mut sim_a = Simulation::new();
        let mut sim_b = Simulation::new();
        let mut sys_a = fake_system(IVec3::ZERO);
        let mut sys_b = fake_system(IVec3::ZERO);
        let make_p = |counter: u8| Particle {
            type_id: ParticleTypeId(0),
            coords: IVec3::ZERO,
            origin: IVec3::ZERO,
            direction: [SimFixed::from_num(0); 3],
            velocity: SimFixed::from_num(0),
            lifetime_remaining: 100,
            damage_counter: 0,
            state_ai_advance: 4,
            animation_state: 0,
            translucency: 0,
            marked_for_deletion: false,
            drift_x: 0,
            drift_y: 0,
            drift_z: 0,
            spark: None,
            prev_delta: [SimFixed::from_num(0); 3],
            state_advance_counter: counter,
        };
        sys_a.particles.push(make_p(0));
        sys_b.particles.push(make_p(3));
        insert_system(&mut sim_a, sys_a);
        insert_system(&mut sim_b, sys_b);
        assert_ne!(
            sim_a.state_hash(),
            sim_b.state_hash(),
            "state_advance_counter must affect state hash"
        );
    }

    fn particle_with_spark(spark: Option<SparkRuntimeState>) -> Particle {
        Particle {
            type_id: ParticleTypeId(0),
            coords: IVec3::new(-1, 2, 3),
            origin: IVec3::ZERO,
            direction: [SimFixed::from_num(0); 3],
            velocity: SimFixed::from_num(0),
            lifetime_remaining: 9,
            damage_counter: 0,
            state_ai_advance: 0,
            animation_state: 0,
            translucency: 0,
            marked_for_deletion: false,
            drift_x: 0,
            drift_y: 0,
            drift_z: 0,
            spark,
            prev_delta: [SimFixed::from_num(0); 3],
            state_advance_counter: 0,
        }
    }

    fn hash_with_particle(particle: Particle) -> u64 {
        let mut sim = Simulation::new();
        let mut system = fake_system(IVec3::ZERO);
        system.particles.push(particle);
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
        base.particles.push(particle_with_spark(None));
        let base_hash = hash_with_system(base.clone());
        let owner_house = Simulation::new().interner.intern("Americans");
        let edits: Vec<Box<dyn Fn(&mut ParticleSystem)>> = vec![
            Box::new(|s| s.offset = IVec3::new(1, 0, 0)),
            Box::new(|s| s.spawn_timer = SimFixed::from_num(1)),
            Box::new(|s| s.spark_spawn_frames = 1),
            Box::new(|s| s.attached_entity = Some(7)),
            Box::new(|s| s.owner_entity = Some(7)),
            Box::new(|s| s.target_coords = IVec3::new(0, 1, 0)),
            Box::new(move |s| s.owner_house = Some(owner_house)),
            Box::new(|s| s.particles[0].origin = IVec3::new(0, 0, 1)),
            Box::new(|s| s.particles[0].direction[1] = SimFixed::from_num(1)),
            Box::new(|s| s.particles[0].velocity = SimFixed::from_num(1)),
            Box::new(|s| s.particles[0].damage_counter = 1),
            Box::new(|s| s.particles[0].state_ai_advance = 1),
            Box::new(|s| s.particles[0].drift_x = 1),
            Box::new(|s| s.particles[0].drift_y = 1),
            Box::new(|s| s.particles[0].drift_z = 1),
            Box::new(|s| s.particles[0].prev_delta[2] = SimFixed::from_num(1)),
        ];
        for (index, edit) in edits.iter().enumerate() {
            let mut system = base.clone();
            edit(&mut system);
            assert_ne!(base_hash, hash_with_system(system), "edit {index}");
        }
    }

    #[test]
    fn spark_coordinate_lifetime_and_delete_state_remain_hashed() {
        let state = SparkRuntimeState {
            velocity_x: NativeF32Bits::POSITIVE_ZERO,
            velocity_y: NativeF32Bits::POSITIVE_ZERO,
            velocity_z: NativeF32Bits::POSITIVE_ZERO,
            start_rgb: [0; 3],
            color_index: 0,
            color_accumulator: NativeF64Bits::POSITIVE_ZERO,
        };
        let base = particle_with_spark(Some(state));
        let base_hash = hash_with_particle(base.clone());

        let mut changed = base.clone();
        changed.coords.x = 0;
        assert_ne!(base_hash, hash_with_particle(changed));

        let mut changed = base.clone();
        changed.lifetime_remaining = 8;
        assert_ne!(base_hash, hash_with_particle(changed));

        let mut changed = base;
        changed.marked_for_deletion = true;
        assert_ne!(base_hash, hash_with_particle(changed));
    }

    #[test]
    fn terrain_animations_included_in_state_hash() {
        use crate::sim::terrain_spawn::TerrainAnimationState;

        let mut sim_a = Simulation::new();
        let sim_b = Simulation::new();
        let type_ref = sim_a.interner.intern("TIBTRE01");
        sim_a.production.terrain_animations.insert(
            (10, 10),
            TerrainAnimationState::new(
                type_ref,
                crate::util::native_x87::NativeF32Bits::from_bits(0x3b44_9ba6),
                3,
                22,
                0,
            ),
        );

        assert_ne!(
            sim_a.state_hash(),
            sim_b.state_hash(),
            "terrain_animations must affect state hash",
        );
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
            .production
            .terrain_animations
            .insert((10, 10), state.clone());
        sim_b.production.terrain_animations.insert((10, 10), state);
        assert_eq!(sim_a.state_hash(), sim_b.state_hash());

        let spawner_b = sim_b
            .production
            .terrain_animations
            .get_mut(&(10, 10))
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
mod tube_movement_hash_tests {
    use super::Simulation;
    use crate::map::tube_facts::TubeId;
    use crate::sim::components::{DriveCoord, Health};
    use crate::sim::game_entity::GameEntity;
    use crate::sim::movement::tube_movement::LowBridgeTubeMovementState;

    fn fixture_entity() -> GameEntity {
        GameEntity::new_at_frame_zero_for_test(
            1,
            0,
            0,
            0,
            0,
            crate::sim::intern::test_intern("Allies"),
            Health { current: 100 },
            crate::sim::intern::test_intern("MTNK"),
            crate::map::entities::EntityCategory::Unit,
            0,
            5,
            true,
        )
    }

    fn hash_entity(entity: GameEntity) -> u64 {
        let mut sim = Simulation::new();
        sim.substrate.entities.insert(entity);
        sim.state_hash()
    }

    #[test]
    fn gsi_04_15_exact_z_and_live_tube_payload_are_fully_hashed() {
        let fixture = fixture_entity();
        let default_hash = hash_entity(fixture.clone());

        let mut exact_z = fixture.clone();
        exact_z.position.exact_z_leptons = Some(-37);
        let exact_z_hash = hash_entity(exact_z.clone());
        assert_ne!(default_hash, exact_z_hash);
        exact_z.position.exact_z_leptons = Some(-36);
        assert_ne!(exact_z_hash, hash_entity(exact_z));

        let state = LowBridgeTubeMovementState {
            tube_id: TubeId(3),
            cursor: 1,
            target: DriveCoord {
                x: 640,
                y: 128,
                z: -19,
            },
        };
        let mut active = fixture;
        active.position.exact_z_leptons = Some(-37);
        active.low_bridge_tube_state = Some(state);
        let active_hash = hash_entity(active.clone());
        assert_ne!(exact_z_hash, active_hash);

        let variants = [
            LowBridgeTubeMovementState {
                tube_id: TubeId(4),
                ..state
            },
            LowBridgeTubeMovementState { cursor: 2, ..state },
            LowBridgeTubeMovementState {
                target: DriveCoord {
                    x: 641,
                    ..state.target
                },
                ..state
            },
            LowBridgeTubeMovementState {
                target: DriveCoord {
                    y: 129,
                    ..state.target
                },
                ..state
            },
            LowBridgeTubeMovementState {
                target: DriveCoord {
                    z: -18,
                    ..state.target
                },
                ..state
            },
        ];
        for variant in variants {
            active.low_bridge_tube_state = Some(variant);
            assert_ne!(active_hash, hash_entity(active.clone()));
        }
    }
}

#[cfg(test)]
mod radio_contact_hash_tests {
    use super::Simulation;
    use crate::map::entities::EntityCategory;
    use crate::sim::components::Health;
    use crate::sim::game_entity::GameEntity;

    fn vehicle_entity(sim: &mut Simulation, id: u64) -> GameEntity {
        GameEntity::new_at_frame_zero_for_test(
            id,
            10,
            10,
            0,
            0,
            sim.interner.intern("Americans"),
            Health { current: 100 },
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
            .runtime
            .current_frame += 1;
        assert_ne!(present, sim.state_hash());
        let advanced = sim.state_hash();
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .building_damage_state_active = true;
        assert_ne!(advanced, sim.state_hash());
        let retained = sim.state_hash();
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .building_anim_slots
            .swap(3, 4);
        assert_ne!(retained, sim.state_hash());
    }
}

#[cfg(test)]
mod infantry_hash_tests {
    use super::Simulation;
    use crate::map::entities::EntityCategory;
    use crate::sim::animation::{Animation, SequenceKind};
    use crate::sim::components::Health;
    use crate::sim::game_entity::{GameEntity, InfantryRuntime};

    fn hash_entity(sim: &mut Simulation, category: EntityCategory) -> GameEntity {
        GameEntity::new_at_frame_zero_for_test(
            1,
            0,
            0,
            0,
            0,
            sim.interner.intern("Allies"),
            Health { current: 100 },
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
        sequence.sequence = SequenceKind::Attack;
        assert_ne!(base_hash, hash(Some(sequence)));

        let mut frame_index = base.clone();
        frame_index.frame_index = 1;
        assert_ne!(base_hash, hash(Some(frame_index)));

        let mut elapsed_frames = base.clone();
        elapsed_frames.elapsed_frames = 1;
        assert_ne!(base_hash, hash(Some(elapsed_frames)));

        let mut finished = base;
        finished.finished = true;
        assert_ne!(base_hash, hash(Some(finished)));
    }

    #[test]
    fn infantry_presentation_animation_cannot_change_simulation_hash() {
        // The class owns Doing/Stage. A retained presentation component after
        // restore cannot become a second simulation clock through hashing.
        let absent = hash_with_animation(EntityCategory::Infantry, None);
        let mut animation = Animation::new(SequenceKind::Die1);
        animation.frame_index = u16::MAX;
        animation.elapsed_frames = u16::MAX;
        animation.finished = true;
        assert_eq!(
            absent,
            hash_with_animation(EntityCategory::Infantry, Some(animation))
        );
    }

    #[test]
    fn infantry_fear_and_prone_change_hash() {
        let mut sim_a = Simulation::new();
        let mut sim_b = Simulation::new();
        let mut a = hash_entity(&mut sim_a, EntityCategory::Infantry);
        let b = hash_entity(&mut sim_b, EntityCategory::Infantry);
        a.infantry = Some(InfantryRuntime {
            fear_level: 10,
            is_prone: false,
            ..InfantryRuntime::new()
        });
        sim_a.substrate.entities.insert(a);
        sim_b.substrate.entities.insert(b);
        assert_ne!(sim_a.state_hash(), sim_b.state_hash());

        let mut sim_a = Simulation::new();
        let mut sim_b = Simulation::new();
        let mut a = hash_entity(&mut sim_a, EntityCategory::Infantry);
        let b = hash_entity(&mut sim_b, EntityCategory::Infantry);
        a.infantry = Some(InfantryRuntime {
            fear_level: 0,
            is_prone: true,
            ..InfantryRuntime::new()
        });
        sim_a.substrate.entities.insert(a);
        sim_b.substrate.entities.insert(b);
        assert_ne!(sim_a.state_hash(), sim_b.state_hash());
    }

    #[test]
    fn infantry_cell_entry_blocked_changes_hash_only_when_set() {
        let mut sim_a = Simulation::new();
        let mut sim_b = Simulation::new();
        let mut a = hash_entity(&mut sim_a, EntityCategory::Infantry);
        let b = hash_entity(&mut sim_b, EntityCategory::Infantry);
        a.infantry = Some(InfantryRuntime {
            cell_entry_blocked: true,
            ..InfantryRuntime::new()
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

    #[test]
    fn foot_path_runtime_hash_and_snapshot_survive_without_movement_adapter() {
        use crate::sim::components::FootPathRuntime;
        use crate::sim::timer::CdTimer;
        let mut sim = Simulation::new();
        let actor = hash_entity(&mut sim, EntityCategory::Infantry);
        sim.substrate.entities.insert(actor);
        let before = sim.state_hash();
        let mut retained = FootPathRuntime::at_frame(0);
        retained.movement_timer = CdTimer::from_raw(-1, -7);
        retained.blocked_timer = CdTimer::from_raw(i32::MAX - 2, 31);
        retained.path_blocked = true;
        retained.retries_left = u32::MAX;
        sim.substrate
            .entities
            .get_mut(1)
            .unwrap()
            .navigation
            .path_runtime = retained;
        assert_ne!(sim.state_hash(), before);
        let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "foot-path", 0);
        let loaded = crate::sim::snapshot::GameSnapshot::load(&bytes)
            .unwrap()
            .sim;
        let actor = loaded.entities().get(1).unwrap();
        assert!(actor.movement_target.is_none());
        assert_eq!(actor.navigation.path_runtime, retained);
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
        assert_eq!(actor.coerce_foot_threat_mask(input_mask), effective_mask);
        actor.finish_foot_threat_scan(false);
        assert!(!actor.foot_retarget_after_stop());
        assert_eq!(actor.coerce_foot_threat_mask(input_mask), input_mask);
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
                    Health { current: 100 },
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
        assert_eq!(actor.navigation.path_runtime.scold_latch_raw(), 0);
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
                .path_runtime
                .set_scold_latch_for_test(raw);
            let current_hash = sim.state_hash();
            assert!(!retained_hashes.contains(&current_hash) || raw == 0);
            retained_hashes.push(current_hash);

            let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "scold", 0);
            let mut restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
                .unwrap()
                .sim;
            let actor = restored.substrate.entities.get(1).unwrap();
            assert!(actor.movement_target.is_none());
            assert_eq!(
                u64::from(actor.navigation.path_runtime.scold_latch_raw()),
                row["after_original_noinit_constructor"].as_u64().unwrap()
            );
            assert_eq!(restored.state_hash(), current_hash);
            let path = &mut restored
                .substrate
                .entities
                .get_mut(1)
                .unwrap()
                .navigation
                .path_runtime;
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
}

#[cfg(test)]
mod native_frame_tests {
    use super::Simulation;

    #[test]
    fn one_advance_is_one_native_frame_for_any_host_duration() {
        let mut sim = Simulation::new();
        let host_durations = [0, 1, 22, 67, 1_000, u32::MAX];
        for (index, tick_ms) in host_durations.into_iter().enumerate() {
            sim.advance_tick(&[], None, None, None, tick_ms);
            assert_eq!(sim.session.binary_frame, index as u32 + 1);
        }
    }

    #[test]
    fn native_frame_wraps_after_u32_max() {
        let mut sim = Simulation::new();
        sim.session.binary_frame = u32::MAX;

        sim.advance_tick(&[], None, None, None, 22);

        assert_eq!(sim.session.binary_frame, 0);
    }

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
        sim_a.session.total_sim_ms = 123_456;

        assert_eq!(sim_a.state_hash(), sim_b.state_hash());
    }
}

#[cfg(test)]
mod rocking_hash_tests {
    use super::Simulation;
    use crate::map::entities::EntityCategory;
    use crate::sim::components::{Health, RockingState};
    use crate::sim::game_entity::GameEntity;
    use crate::util::fixed_math::SimFixed;

    fn make_sim_with_one_vehicle() -> Simulation {
        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Americans");
        let type_id = sim.interner.intern("HTNK");
        let id = sim.substrate.next_stable_object_id;
        sim.substrate.next_stable_object_id += 1;
        let e = GameEntity::new_at_frame_zero_for_test(
            id,
            10,
            10,
            0,
            0,
            owner,
            Health { current: 400 },
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
    fn rocking_state_contributes_to_hash() {
        let a = make_sim_with_one_vehicle();
        let b = make_sim_with_one_vehicle();
        assert_eq!(a.state_hash(), b.state_hash());

        // Mutate only the rocking state of one — hashes must diverge.
        let mut a = a;
        let id = a.substrate.entities.values().next().unwrap().stable_id;
        a.substrate.entities.get_mut(id).unwrap().rocking = Some(RockingState {
            angle_sideways: SimFixed::lit("0.1"),
            ..Default::default()
        });
        assert_ne!(a.state_hash(), b.state_hash());
    }

    #[test]
    fn rocking_velocity_contributes_to_hash() {
        let mut a = make_sim_with_one_vehicle();
        let mut b = make_sim_with_one_vehicle();
        let id_a = a.substrate.entities.values().next().unwrap().stable_id;
        let id_b = b.substrate.entities.values().next().unwrap().stable_id;
        a.substrate.entities.get_mut(id_a).unwrap().rocking = Some(RockingState {
            vel_sideways: SimFixed::lit("0.01"),
            ..Default::default()
        });
        b.substrate.entities.get_mut(id_b).unwrap().rocking = Some(RockingState {
            vel_sideways: SimFixed::lit("0.02"),
            ..Default::default()
        });
        assert_ne!(a.state_hash(), b.state_hash());
    }

    #[test]
    fn rocking_none_vs_default_contributes_to_hash() {
        let mut a = make_sim_with_one_vehicle();
        let b = make_sim_with_one_vehicle();
        let id = a.substrate.entities.values().next().unwrap().stable_id;
        a.substrate.entities.get_mut(id).unwrap().rocking = Some(RockingState::default());
        // a has Some(default), b has None — hashes must diverge.
        assert_ne!(a.state_hash(), b.state_hash());
    }
}

#[cfg(test)]
mod c4_hash_tests {
    use super::Simulation;
    use crate::map::entities::EntityCategory;
    use crate::sim::components::{C4PlantState, Health, PendingC4Detonation};
    use crate::sim::game_entity::GameEntity;

    #[test]
    fn c4_state_changes_hash() {
        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Americans");
        let type_id = sim.interner.intern("GHOST");
        let id = sim.substrate.next_stable_object_id;
        sim.substrate.next_stable_object_id += 1;
        let e = GameEntity::new_at_frame_zero_for_test(
            id,
            10,
            10,
            0,
            0,
            owner,
            Health { current: 125 },
            type_id,
            EntityCategory::Infantry,
            0,
            5,
            false,
        );
        sim.substrate.entities.insert(e);
        let h_initial = sim.state_hash();

        // Mutate c4_plant — hash must change.
        sim.substrate.entities.get_mut(id).unwrap().c4_plant = Some(C4PlantState {
            target_building_id: 99,
        });
        let h_with_plant = sim.state_hash();
        assert_ne!(h_initial, h_with_plant, "c4_plant must affect state hash");

        // Mutate pending_c4_detonation — hash must change again.
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .pending_c4_detonation = Some(PendingC4Detonation {
            timer: crate::sim::timer::CdTimer::started(100, 30),
            source_entity_id: Some(7),
        });
        let h_with_pending = sim.state_hash();
        assert_ne!(
            h_with_plant, h_with_pending,
            "pending_c4_detonation must affect state hash"
        );
    }
}

#[cfg(test)]
mod passenger_cargo_hash_tests {
    use super::Simulation;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::passenger::{PassengerCargo, PassengerRole};

    fn sim_with_sizes(first_size: u32, second_size: u32) -> Simulation {
        let mut sim = Simulation::new();
        let mut carrier = GameEntity::test_default(1, "BFRT", "Allied", 5, 5);
        let mut cargo = PassengerCargo::new(5, 0);
        cargo.board_forced(10, first_size);
        cargo.board_forced(11, second_size);
        carrier.passenger_role = PassengerRole::Transport { cargo };
        sim.substrate.entities.insert(carrier);
        sim
    }

    #[test]
    fn per_entry_size_mapping_changes_hash_even_when_total_matches() {
        let a = sim_with_sizes(1, 3);
        let b = sim_with_sizes(3, 1);

        assert_ne!(a.state_hash(), b.state_hash());
    }
}

#[cfg(test)]
mod bridge161_hash_projection_tests {
    use super::*;
    use crate::map::entities::EntityCategory;
    use crate::rules::locomotor_type::LocomotorKind;
    use crate::sim::components::{DriveCoord, Health};
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
            Health { current: 100 },
            type_ref,
            EntityCategory::Infantry,
            0,
            5,
            true,
        );
        let mut locomotor = LocomotorState::for_test_kind(kind);
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
                0 => state.head = Some(coord),
                1 => state.destination = Some(coord),
                2 => state.moving = true,
                3 => state.animation_moving = true,
                _ => unreachable!(),
            },
            LocomotorRuntimePayload::Hover(state) => state.set_head(Some(coord)),
            LocomotorRuntimePayload::Jumpjet(state) => {
                *state = match field {
                    0 => state.clone().with_destination_for_test(coord),
                    1 => state.clone().with_moving_for_test(true),
                    2 => state.clone().with_phase_for_test(2),
                    3 => {
                        let mut flight = state.flight();
                        flight.target_height = 7;
                        state.clone().with_flight_for_test(flight)
                    }
                    4 => {
                        let mut flight = state.flight();
                        flight.current_speed_bits = 1.0f64.to_bits();
                        state.clone().with_flight_for_test(flight)
                    }
                    5 => {
                        let mut params = state.params();
                        params.speed = 99;
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
                        loco.piggyback.as_mut().unwrap().suspended_mut_for_test()
                    } else {
                        loco
                    };
                    mutate(&mut object.runtime_payload, field);
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
