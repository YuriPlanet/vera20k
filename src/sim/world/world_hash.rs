//! Deterministic state hashing for the Simulation.
//!
//! Produces a reproducible u64 hash over the entire simulation state:
//! tick counter, RNG state, production queues, fog-of-war, entity components.
//! Used for replay verification and desync detection in multiplayer.
//!
//! Dependency rules: same as sim/ (depends on rules/, map/; never render/ui/audio/net).

use std::hash::{Hash, Hasher};

use super::Simulation;
use super::hash_schema::{HashFeature, HashSchema};

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

/// Fold the versioned direct House CRC fields around CurrentIQ.
///
/// gamemd-derived: raw House CRC `0x00502D60..0x0050303F` folds Production at
/// `0x00502E58`, AutocreateAllowed at `0x00502E66`, AITriggersActive at
/// `0x00502E74`, and CurrentIQ at `0x00502E90`; exhaustive census finds no
/// direct AutoBaseBuilding (`House+0x1F3`) feed. Schema v112 retains its
/// committed CurrentIQ-before-two-latches stream, while v113 uses native order.
fn hash_house_ai_activation_fields(
    house: &crate::sim::house_state::HouseState,
    include_house_deploy_latches_v112: bool,
    include_house_update_activation_v113: bool,
    hasher: &mut impl Hasher,
) {
    if !include_house_update_activation_v113 {
        house.current_iq.hash(hasher);
    }
    if include_house_deploy_latches_v112 {
        house.ai_activation.production.hash(hasher);
    }
    if include_house_update_activation_v113 {
        house.ai_activation.autocreate_allowed.hash(hasher);
    }
    if include_house_deploy_latches_v112 {
        house.ai_activation.ai_triggers_active.hash(hasher);
    }
    if include_house_update_activation_v113 {
        house.current_iq.hash(hasher);
    }
}

#[cfg(test)]
mod drive_ship_slope_hash_tests {
    use super::Simulation;
    use crate::map::entities::EntityCategory;
    use crate::rules::locomotor_type::LocomotorKind;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::movement::locomotion::LocomotorRuntimePayload;
    use crate::sim::movement::locomotor::{LocomotorState, MovementLayer};
    use crate::sim::movement::slope_transition::SlopeTransitionState;

    fn hash_with_state(kind: LocomotorKind, stashed: bool, state: SlopeTransitionState) -> u64 {
        let mut sim = Simulation::new();
        let mut entity = GameEntity::test_default(1, "SLOPE", "Americans", 2, 2);
        entity.category = EntityCategory::Unit;
        let mut locomotor = LocomotorState::for_test_kind(kind);
        locomotor.runtime_payload = match kind {
            LocomotorKind::Drive => LocomotorRuntimePayload::Drive(state),
            LocomotorKind::Ship => LocomotorRuntimePayload::Ship(state),
            _ => unreachable!(),
        };
        if stashed {
            assert!(locomotor.begin_piggyback(LocomotorKind::Teleport, MovementLayer::Ground, 90,));
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
    fn gsi_04_07_hashes_live_dummy_overlay_identity_and_state() {
        let sim = Simulation::new();
        let dummy = sim.effective_shared_cell_dummy();
        let clear_hash = sim.state_hash();
        let v114_hash = sim.state_hash_without_wall_runtime_v115();

        dummy.write_overlay_identity(0x02);
        let identity_hash = sim.state_hash();
        assert_ne!(
            clear_hash, identity_hash,
            "dummy OverlayType identity changes later wall lookup behavior"
        );

        dummy.write_overlay_state(0x2a);
        assert_ne!(
            identity_hash,
            sim.state_hash(),
            "dummy OverlayData damage/connectivity is independent hash authority"
        );
        assert_eq!(
            v114_hash,
            sim.state_hash_without_wall_runtime_v115(),
            "the v114 provenance schema excludes both dummy overlay fields"
        );
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
        let v106_clear_hash = sim.state_hash_without_spark_dummy_level_slope_v107();

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
            "dummy level alone is unconditional v107 hash authority"
        );
        assert_eq!(
            v106_clear_hash,
            sim.state_hash_without_spark_dummy_level_slope_v107(),
            "the v106 provenance schema excludes an unretained dummy level change"
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
            "dummy slope alone is unconditional v107 hash authority"
        );
        assert_eq!(
            v106_clear_hash,
            sim.state_hash_without_spark_dummy_level_slope_v107(),
            "the v106 provenance schema excludes an unretained dummy slope change"
        );
    }
}

#[cfg(test)]
mod real_cell_bridge_hash_schema_tests {
    use super::Simulation;
    use crate::map::resolved_terrain::ResolvedTerrainGrid;

    #[test]
    fn gsi_04_01_real_cell_bridge_authority_is_current_schema_only() {
        let baseline = Simulation::new();
        let mut different_authority = Simulation::new();
        different_authority.install_resolved_terrain_for_new_map(ResolvedTerrainGrid::from_cells(
            0,
            1,
            Vec::new(),
        ));

        assert_ne!(
            baseline.state_hash(),
            different_authority.state_hash(),
            "current schema hashes the exact real-cell bridge authority, including its aligned shape"
        );
        assert_eq!(
            baseline.state_hash_before_lifecycle_v28_and_mission_v29(),
            different_authority.state_hash_before_lifecycle_v28_and_mission_v29(),
            "the pre-v28 provenance schema excludes both the v90 tag and authority"
        );
        assert_eq!(
            baseline.state_hash_without_mission_v29(),
            different_authority.state_hash_without_mission_v29(),
            "the pre-v29 provenance schema excludes both the v90 tag and authority"
        );
    }
}

/// The two removed Option payloads could disagree with the retained class.
/// Only independently established absent-payload, pending=false fixtures have
/// a projection.
/// Reject current evidence of an active or partially retired track, rather
/// than inventing the former geometry/cursor/speed. A destination or retained
/// residual alone is not an active track: native keeps those independently.
#[cfg(test)]
fn assert_retired_track_projection_is_bounded(entity: &crate::sim::game_entity::GameEntity) {
    let idle_progress = |track: &crate::sim::components::TrackProgress| {
        track.turn_index == -1 && matches!(track.cursor, -1 | 0)
    };
    let drive_absent = entity.drive_locomotion.as_ref().is_none_or(|drive| {
        idle_progress(&drive.track)
            && drive.head_to.is_none()
            && !drive.track_valid
            && !drive.turn_latched
            && drive.occupation_head_to.is_none()
            && drive.occupation_handoff.is_none()
    });
    let ship_absent = entity.ship_locomotion.as_ref().is_none_or(|ship| {
        !ship.track_valid
            && !ship.turn_latched
            && idle_progress(&ship.track)
            && ship.head_to.is_none()
            && ship.occupation_head_to.is_none()
            && ship.occupation_handoff.is_none()
    });
    assert!(
        drive_absent && ship_absent,
        "pre-v167 detached-track projection requires an independently established absent payload; entity {} retains active track state that cannot recover the obsolete representation",
        entity.stable_id(),
    );
}

/// Schema166 derived Hash included a pending=false field immediately after
/// TrackProgress. References hash their values; deriving these former payload
/// layouts and mapping the original Options preserves both field order and
/// Option discriminants. This bounded fixture projection cannot recover an
/// old pending=true obligation or either removed detached executor.
#[cfg(test)]
fn hash_retained_track_classes_before_167(
    entity: &crate::sim::game_entity::GameEntity,
    hasher: &mut impl Hasher,
) {
    use crate::sim::components::{
        DriveCoord, DriveOccupationFootprint, DriveTurnState, TrackProgress,
    };
    use crate::util::fixed_math::SimFixed;

    #[derive(Hash)]
    struct DriveSchema166<'a> {
        destination: &'a Option<DriveCoord>,
        head_to: &'a Option<DriveCoord>,
        turn: &'a DriveTurnState,
        track: &'a TrackProgress,
        pending_track_occupation: bool,
        end_permitted: &'a bool,
        track_valid: &'a bool,
        target_speed_fraction: &'a SimFixed,
        occupation_head_to: &'a Option<DriveOccupationFootprint>,
        occupation_handoff: &'a Option<DriveOccupationFootprint>,
    }
    #[derive(Hash)]
    struct ShipSchema166<'a> {
        destination: &'a Option<DriveCoord>,
        head_to: &'a Option<DriveCoord>,
        track: &'a TrackProgress,
        pending_track_occupation: bool,
        target_speed_fraction: &'a SimFixed,
        occupation_head_to: &'a Option<DriveOccupationFootprint>,
        occupation_handoff: &'a Option<DriveOccupationFootprint>,
    }

    entity
        .drive_locomotion
        .as_ref()
        .map(|drive| DriveSchema166 {
            destination: &drive.destination,
            head_to: &drive.head_to,
            turn: &drive.turn,
            track: &drive.track,
            pending_track_occupation: false,
            end_permitted: &drive.end_permitted,
            track_valid: &drive.track_valid,
            target_speed_fraction: &drive.target_speed_fraction,
            occupation_head_to: &drive.occupation_head_to,
            occupation_handoff: &drive.occupation_handoff,
        })
        .hash(hasher);
    entity
        .ship_locomotion
        .as_ref()
        .map(|ship| ShipSchema166 {
            destination: &ship.destination,
            head_to: &ship.head_to,
            track: &ship.track,
            pending_track_occupation: false,
            target_speed_fraction: &ship.target_speed_fraction,
            occupation_head_to: &ship.occupation_head_to,
            occupation_handoff: &ship.occupation_handoff,
        })
        .hash(hasher);
}

fn hash_retained_track_classes(
    entity: &crate::sim::game_entity::GameEntity,
    _schema: HashSchema,
    hasher: &mut impl Hasher,
) {
    #[cfg(test)]
    if !_schema.includes(HashFeature::TrackAuthority) {
        hash_retained_track_classes_before_167(entity, hasher);
        return;
    }
    entity.drive_locomotion.hash(hasher);
    entity.ship_locomotion.hash(hasher);
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

/// Reconstruct the pre-v29 Mission hash pre-image for the committed regression
/// provenance fixtures.
///
/// The old schema could not represent unknown selectors or a handler state
/// wider than one byte, so this path fails loudly rather than normalizing
/// unrepresentable final-schema state. The fixtures also never retask at frame
/// zero, which makes the old constructor sentinel recoverable. Live hashing
/// never calls this helper.
fn hash_mission_com_before_v29(
    mission: &crate::sim::mission::MissionCom,
    hasher: &mut impl Hasher,
) {
    use crate::sim::mission::{MissionId, MissionType};

    fn known_or_idle(id: MissionId) -> MissionType {
        if id == MissionId::NONE {
            MissionType::None
        } else {
            id.known()
                .expect("pre-v29 Mission hash cannot represent an unknown selector")
        }
    }

    fn hash_legacy_optional(id: MissionId, hasher: &mut impl Hasher) {
        if id == MissionId::NONE {
            0u8.hash(hasher);
        } else {
            1u8.hash(hasher);
            (known_or_idle(id) as u16).hash(hasher);
        }
    }

    (known_or_idle(mission.current()) as u16).hash(hasher);
    hash_legacy_optional(mission.queued(), hasher);
    hash_legacy_optional(mission.suspended(), hasher);
    u8::try_from(mission.handler_state())
        .expect("pre-v29 Mission hash cannot represent a handler state above 255")
        .hash(hasher);
    let legacy_start = if mission.dispatch_timer().start_frame() == 0
        && mission.dispatch_timer().delay() == 0
        && mission.mission_start_frame() == 0
    {
        // These provenance fixtures never retask at frame zero. Final-schema
        // construction anchors dispatch there, while the old frame-free
        // constructor represented the same untouched compatibility state with
        // MissionTimer's u32::MAX sentinel.
        u32::MAX
    } else {
        mission.dispatch_timer().start_frame() as u32
    };
    legacy_start.hash(hasher);
    (mission.dispatch_timer().delay() as u32).hash(hasher);
    mission.ai_counter().hash(hasher);
}

fn hash_mission_leaf(leaf: &crate::sim::mission::MissionLeafState, hasher: &mut impl Hasher) {
    if let Some(unit) = leaf.as_unit() {
        0u8.hash(hasher);
        unit.deploy_begin_active().hash(hasher);
        unit.deploy_reverse_active().hash(hasher);
        unit.tracker_byte_18().hash(hasher);
        unit.tracker_byte_19().hash(hasher);
    } else if let Some(infantry) = leaf.as_infantry() {
        1u8.hash(hasher);
        infantry.firing_sequence_latch().hash(hasher);
        infantry.doing().hash(hasher);
    } else if let Some(aircraft) = leaf.as_aircraft() {
        2u8.hash(hasher);
        aircraft.action_latch().hash(hasher);
        aircraft.transition_ready_latch().hash(hasher);
        aircraft.airstrike_manager_present().hash(hasher);
    } else if let Some(building) = leaf.as_building() {
        3u8.hash(hasher);
        building.ready_latch().hash(hasher);
    }
}

impl Simulation {
    /// Deterministic state hash over canonicalized simulation state.
    ///
    /// Hashes clocks, Scenario RNG, production, fog, alliances, and all entity
    /// components in stable-entity-ID order (EntityStore keys_sorted) for determinism.
    pub fn state_hash(&self) -> u64 {
        self.state_hash_with_schema(HashSchema::Current)
    }

    /// Test-only schema166 provenance for fixtures whose two detached track
    /// options were independently established to be absent and both pending
    /// flags false. Restores those positional fields without reconstructing
    /// removed runtime state.
    /// Active retained tracks are rejected; neither their former ordinary
    /// geometry copy nor a separate forced executor can be recovered here.
    #[cfg(test)]
    pub(crate) fn state_hash_without_track_authority_v167(&self) -> u64 {
        self.state_hash_with_schema(HashSchema::Before(167))
    }

    /// Project out schema159's Cell lists/Sight0 and later gated layouts.
    /// This does not reverse semantic changes: raw Infantry house IDs cannot
    /// reconstruct the former entity-ID owner encoding. Prior pins remain
    /// independent checks; no reverse owner mapping is invented here.
    #[cfg(test)]
    pub(crate) fn state_hash_without_cell_membership_v159(&self) -> u64 {
        self.state_hash_with_schema(HashSchema::Before(159))
    }

    /// Provenance probe for the raw infantry owner identity change (entity id
    /// -> mark-time House index, InfantryClass::MarkCellOccupancy 0x005217C0):
    /// the schema-160 composition with the raw owner fields excluded. Equal
    /// values across the change prove every other fold is unchanged.
    #[cfg(test)]
    pub(crate) fn state_hash_without_raw_infantry_owners_v161_probe(&self) -> u64 {
        self.state_hash_with_schema(HashSchema::BeforeWithoutRawInfantryOwners(161))
    }

    /// Test-only provenance probe for the schema160 Foot path runtime
    /// rebaseline: the retained timers/latch/count are projected back into the
    /// former positional MovementTarget encoding instead of hashed as an owner.
    #[cfg(test)]
    pub(crate) fn state_hash_without_foot_path_runtime_v160(&self) -> u64 {
        self.state_hash_with_schema(HashSchema::Before(160))
    }

    /// Test-only provenance probe for the schema161 bridge locomotor/raw
    /// Dummy rebaseline: projects out only the schema161 payload and Dummy
    /// composition. It cannot reverse the raw Infantry owner identity change.
    #[cfg(test)]
    pub(crate) fn state_hash_without_bridge_locomotor_v161(&self) -> u64 {
        self.state_hash_with_schema(HashSchema::Before(161))
    }

    /// Test-only provenance probe for the v29 Mission hash rebaseline.
    ///
    /// It retains lifecycle-v28 fields and reconstructs the exact prior
    /// Mission/hash layout from representable final state.
    #[cfg(test)]
    pub(crate) fn state_hash_without_mission_v29(&self) -> u64 {
        self.state_hash_with_schema(HashSchema::Before(29))
    }

    /// Test-only provenance probe for the historical pre-v28 baseline.
    ///
    /// Both the lifecycle-v28 and Mission-v29 additions are omitted so later
    /// schema changes do not invalidate that earlier proof.
    #[cfg(test)]
    pub(crate) fn state_hash_before_lifecycle_v28_and_mission_v29(&self) -> u64 {
        self.state_hash_with_schema(HashSchema::Before(28))
    }

    /// Test-only provenance probe for the v107 unconditional Spark dummy
    /// level/slope fold. It reconstructs the committed v106 hash layout.
    #[cfg(test)]
    pub(crate) fn state_hash_without_spark_dummy_level_slope_v107(&self) -> u64 {
        self.state_hash_with_schema(HashSchema::Before(107))
    }

    /// Test-only provenance probe for the v109 naval BuildConst folds. It
    /// reconstructs the committed v108 hash layout while retaining every
    /// earlier schema addition.
    #[cfg(test)]
    pub(crate) fn state_hash_without_naval_build_const_v109(&self) -> u64 {
        self.state_hash_with_schema(HashSchema::Before(109))
    }

    /// Test-only provenance probe for the v110 BasePlan folds. It reconstructs
    /// the committed v109 hash layout while retaining every earlier schema
    /// addition, including naval BuildConst order and membership.
    #[cfg(test)]
    pub(crate) fn state_hash_without_base_plan_v110(&self) -> u64 {
        self.state_hash_with_schema(HashSchema::Before(110))
    }

    /// Test-only provenance probe for the schema-v111 BasePlan-center fold.
    #[cfg(test)]
    pub(crate) fn state_hash_without_base_plan_center_v111(&self) -> u64 {
        self.state_hash_with_schema(HashSchema::Before(111))
    }

    /// Test-only provenance probe for the schema-v112 House deploy-latch fold.
    #[cfg(test)]
    pub(crate) fn state_hash_without_house_deploy_latches_v112(&self) -> u64 {
        self.state_hash_with_schema(HashSchema::Before(112))
    }

    /// Test-only provenance probe for the schema-v113 House-update activation
    /// fold. It reconstructs the committed v112 CurrentIQ/latch order.
    #[cfg(test)]
    pub(crate) fn state_hash_without_house_update_activation_v113(&self) -> u64 {
        self.state_hash_with_schema(HashSchema::Before(113))
    }

    /// Test-only provenance probe for the schema-v114 raw crate-slot fold.
    #[cfg(test)]
    pub(crate) fn state_hash_without_crate_authority_v114(&self) -> u64 {
        self.state_hash_with_schema(HashSchema::Before(114))
    }

    /// Test-only provenance probe for the schema-v117 disguise-detect folds:
    /// the `CellClass+0xAC[house]` counter plane and the cached
    /// `DetectDisguiseRange=` deposit radius. It reconstructs the committed
    /// v115 hash layout.
    #[cfg(test)]
    pub(crate) fn state_hash_without_disguise_detect_v117(&self) -> u64 {
        self.state_hash_with_schema(HashSchema::Before(117))
    }

    /// Test-only provenance probe for the schema-v132 `HouseClass+0x242`
    /// harvester no-ore latch fold. It reconstructs the committed v117 layout.
    #[cfg(test)]
    pub(crate) fn state_hash_without_house_harvester_no_ore_v132(&self) -> u64 {
        self.state_hash_with_schema(HashSchema::Before(132))
    }

    /// Test-only provenance probe for the schema-v133 House EVA advice folds
    /// (`+0x57D4` funds timer, `[0xA8F040]` low-power guard). It reconstructs
    /// the committed v132 layout.
    #[cfg(test)]
    pub(crate) fn state_hash_without_house_eva_v133(&self) -> u64 {
        self.state_hash_with_schema(HashSchema::Before(133))
    }

    /// Test-only provenance probe for the schema-v135 credit-income folds
    /// (the `BuildingClass+0x6D0` ProduceCash timer and the
    /// `TechnoClass+0x1CC/+0x1D0` drain link on every entity). It
    /// reconstructs the committed v133/v134 layout.
    #[cfg(test)]
    pub(crate) fn state_hash_without_credit_income_v135(&self) -> u64 {
        self.state_hash_with_schema(HashSchema::Before(135))
    }

    /// Reconstruct the v135 composition before the Infantry terminal-policy fold.
    #[cfg(test)]
    pub(crate) fn state_hash_without_infantry_terminal_v136(&self) -> u64 {
        self.state_hash_with_schema(HashSchema::Before(136))
    }

    /// Test-only provenance probe for the schema-v115 retained wall-count and
    /// shared-dummy overlay folds.
    #[cfg(test)]
    pub(crate) fn state_hash_without_wall_runtime_v115(&self) -> u64 {
        self.state_hash_with_schema(HashSchema::Before(115))
    }

    pub(super) fn state_hash_with_schema(&self, schema: HashSchema) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();

        self.session.tick.hash(&mut hasher);
        self.session.binary_frame.hash(&mut hasher);
        // ScenarioClass owns the sole saved/synchronized RNG. Main and MapGen
        // are process globals and are deliberately absent from this hash.
        self.scenario_rng.hash_state(&mut hasher);
        self.substrate.next_stable_object_id.hash(&mut hasher);
        self.substrate.next_occupancy_enter_order.hash(&mut hasher);
        // YR LogicClass trigger latches are save/lockstep state, even though
        // their camera/message outcomes stay app-owned and are not hashed.
        if schema.includes(HashFeature::MasterFrame) {
            self.trigger_runtime.hash_state(&mut hasher);
            self.team_script_vm
                .hash_state(self.session.binary_frame as i32, &mut hasher);
        }
        if schema.includes(HashFeature::PlayfieldAuthority) {
            self.hash_playfield_authority(&mut hasher);
        }

        // LogicClass active-object order — authoritative (drives reconciliation order).
        let order = self.substrate.logic.as_slice();
        order.len().hash(&mut hasher);
        for id in order {
            id.hash(&mut hasher);
        }
        if schema.includes(HashFeature::DisplayLayers) {
            #[cfg(test)]
            if !schema.includes(HashFeature::AnimationDisplay) {
                self.substrate
                    .display
                    .fold_hash_excluding(&mut hasher, |id| self.substrate.anims.contains_key(id));
            } else {
                self.substrate.display.fold_hash(&mut hasher);
            }
            #[cfg(not(test))]
            self.substrate.display.fold_hash(&mut hasher);
        }

        if schema.includes(HashFeature::Lifecycle) {
            // PendingDeleteList is an independent ordered substrate fact. The
            // length delimiter distinguishes queue boundaries before the ordered
            // IDs are folded (duplicates are intentionally preserved here).
            self.substrate.pending_delete.len().hash(&mut hasher);
            for id in &self.substrate.pending_delete {
                id.hash(&mut hasher);
            }
        }

        self.substrate.fold_raw_cell_occupation(
            &mut hasher,
            schema.includes(HashFeature::BridgeLocomotorAndDummy),
            schema.includes_raw_infantry_owners(),
        );
        self.substrate.fold_hidden_occupation(&mut hasher);
        self.substrate.fold_air_slots(&mut hasher);
        self.substrate.fold_base_reservations(&mut hasher);
        if schema.includes(HashFeature::CellMembership) {
            self.substrate.occupancy.hash_memberships(&mut hasher);
        }

        self.session.fold_game_options(&mut hasher);
        self.hash_houses(&mut hasher, schema);
        if schema.includes(HashFeature::TerminalScore) {
            self.hash_terminal_score_snapshot(&mut hasher);
        }
        self.hash_production(&mut hasher, schema);
        if schema.includes(HashFeature::AircraftDockState) {
            self.production.airfield_docks.hash_state(&mut hasher);
        }
        self.hash_power_states(&mut hasher);
        self.hash_fog_and_alliances(&mut hasher, schema);
        self.hash_bridge_state(&mut hasher);
        if schema.includes(HashFeature::RealCellBridgeFlags) {
            // `resolved_terrain` is derived/skipped. Fold the exact saved real
            // CellClass `0x1180` values once through their serialized authority.
            // Historical pre-v28/pre-v29 provenance probes must omit both this
            // schema tag and the value authority introduced at snapshot v90.
            b"real-cell-bridge-flags-v2".hash(&mut hasher);
            self.real_cell_bridge_flags_0x1180.hash(&mut hasher);
            b"dynamic-terrain-cells-v1".hash(&mut hasher);
            self.dynamic_terrain_cells.hash(&mut hasher);
        }
        self.hash_overlay_grid(&mut hasher, schema);
        if schema.includes(HashFeature::CrateAuthority) {
            self.hash_crate_authority(&mut hasher);
        }
        self.hash_smudge_grid(&mut hasher);
        self.hash_radiation(&mut hasher);
        if schema.includes(HashFeature::MasterFrame) {
            self.hash_projectiles(&mut hasher);
            let shared_dummy_handle = self.effective_shared_cell_dummy();
            let shared_dummy = shared_dummy_handle.snapshot();
            let gap_flags = shared_dummy_handle.retained_bridge_flags() & 0xC00;
            let bridge_keeps_dummy = schema.includes(HashFeature::BridgePublication)
                && (shared_dummy_handle.native_anchor()
                    == Some(crate::map::cell_index::NativeCellIdentity::Dummy)
                    || self.resolved_terrain.as_ref().is_some_and(|terrain| {
                        terrain.iter().any(|cell| {
                            cell.bridge_facts.native_anchor
                                == Some(crate::map::cell_index::NativeCellIdentity::Dummy)
                        })
                    }));
            if schema.includes(HashFeature::BridgePublication) {
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
            if schema.includes(HashFeature::FootNeighborHistory)
                && shared_dummy_handle.neighbor_count() != 0
            {
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
            if schema.includes(HashFeature::SparkDummyLevelSlope) {
                b"shared-cell-dummy-spark-v4".hash(&mut hasher);
                shared_dummy.bridge_flags_0x1180.hash(&mut hasher);
                shared_dummy.level.hash(&mut hasher);
                shared_dummy.slope_type.hash(&mut hasher);
            } else {
                b"shared-cell-dummy-bridge-v3".hash(&mut hasher);
                shared_dummy.bridge_flags_0x1180.hash(&mut hasher);
            }
            // CellClass+0x44/+0x11E can be mutated through true-dummy wall
            // cleanup and later changes lookup-dependent wall behavior. Native
            // Resize reconstructs the process object, so this is synchronized
            // live-state authority rather than Scenario payload authority.
            if schema.includes(HashFeature::WallRuntime) {
                b"shared-cell-dummy-overlay-v1".hash(&mut hasher);
                shared_dummy_overlay.hash(&mut hasher);
            }
            if bridge_keeps_dummy
                || self.projectiles.iter().any(|(_, projectile)| {
                    projectile.target == crate::sim::projectile::ProjectileTarget::DummyCell
                })
            {
                // A retained Bullet pointer additionally makes coordinate
                // deterministic future behavior. Preserve the complete v106
                // field/tag order for historical provenance probes.
                if schema.includes(HashFeature::SparkDummyLevelSlope) {
                    b"shared-cell-dummy-target-v3".hash(&mut hasher);
                    shared_dummy.coord.hash(&mut hasher);
                } else {
                    b"shared-cell-dummy-target-v2".hash(&mut hasher);
                    shared_dummy.coord.hash(&mut hasher);
                    shared_dummy.level.hash(&mut hasher);
                    shared_dummy.slope_type.hash(&mut hasher);
                }
            }
            self.hash_waves(&mut hasher);
        }
        self.hash_super_weapons(&mut hasher);
        self.hash_entities(&mut hasher, schema);
        self.hash_anims(&mut hasher, schema);
        self.hash_voxel_anims(&mut hasher);
        self.hash_particle_systems(&mut hasher);
        self.session.fold_identity(&mut hasher);
        if schema.includes(HashFeature::AnimationAuthority) {
            self.session.pixel_conversion_bounds.hash(&mut hasher);
        }

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
            projectile.payload.owner.index().hash(hasher);
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
            projectile.collision.level_non_water.hash(hasher);
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

    /// Hash per-player house state (BTreeMap = deterministic order).
    fn hash_houses(&self, hasher: &mut impl Hasher, schema: HashSchema) {
        for (owner, house) in &self.houses {
            owner.hash(hasher);
            house.economy.credits.hash(hasher);
            // The sole cash balance retains its original position in the hash stream.
            house.economy.spent_credits.hash(hasher);
            house.economy.harvested_credits.hash(hasher);
            house.economy.purifier_count.hash(hasher);
            house.side_index.hash(hasher);
            house.is_human.hash(hasher);
            house.player_control.hash(hasher);
            (house.difficulty as i32).hash(hasher);
            house.is_defeated.hash(hasher);
            house.has_won.hash(hasher);
            house.has_lost.hash(hasher);
            house.outcome_state.hash(hasher);
            house.map_is_clear.hash(hasher);
            house.spy_sat_active.hash(hasher);
            house.owned_building_count.hash(hasher);
            house.owned_unit_count.hash(hasher);
            house.tech_level.hash(hasher);
            hash_house_ai_activation_fields(
                house,
                schema.includes(HashFeature::HouseDeployLatches),
                schema.includes(HashFeature::HouseUpdateActivation),
                hasher,
            );
            if schema.includes(HashFeature::BaseDefenseResponse) {
                house.strategy_emergency.hash(hasher);
            } else {
                house.strategy_emergency.mode.hash(hasher);
                house.strategy_emergency.all_to_hunt_bias.hash(hasher);
                house
                    .strategy_emergency
                    .last_building_attack_frame
                    .hash(hasher);
            }
            house.grudge_scores.len().hash(hasher);
            for (other, score) in &house.grudge_scores {
                other.hash(hasher);
                score.hash(hasher);
            }
            house.enemy_house.hash(hasher);
            if let Some((rx, ry)) = house.rally_point {
                1u8.hash(hasher);
                rx.hash(hasher);
                ry.hash(hasher);
            } else {
                0u8.hash(hasher);
            }
            if let Some((rx, ry)) = house.base_center {
                1u8.hash(hasher);
                rx.hash(hasher);
                ry.hash(hasher);
            } else {
                0u8.hash(hasher);
            }
            if schema.includes(HashFeature::AlternateBaseCenter) {
                house.alternate_base_center.hash(hasher);
            }
            if schema.includes(HashFeature::NavalBuildConst) && !house.build_const_order.is_empty()
            {
                b"naval-build-const-house-v1".hash(hasher);
                house.build_const_order.len().hash(hasher);
                for stable_id in &house.build_const_order {
                    stable_id.hash(hasher);
                }
            }
            if schema.includes(HashFeature::BasePlan) {
                house.base_plan.percent_built.hash(hasher);
                house.base_plan.nodes.len().hash(hasher);
                for node in &house.base_plan.nodes {
                    node.type_or_control.hash(hasher);
                    node.packed_cell.hash(hasher);
                    node.filled.hash(hasher);
                    node.retry_count.hash(hasher);
                }
            }
            if schema.includes(HashFeature::BasePlanCenter) {
                house.base_plan_center.hash(hasher);
            }
            house.base_reservation.hash(hasher);
            house.waypoint_edge.hash(hasher);
            if schema.includes(HashFeature::HouseHarvesterNoOre) {
                // `HouseClass+0x242`, the sticky harvester no-ore latch — a
                // raw House byte in the native save block and lockstep CRC.
                house.harvester_no_ore.hash(hasher);
            }
            if schema.includes(HashFeature::HouseEva) {
                // `HouseClass+0x57D4` funds-nag TimerStruct and the
                // `[0xA8F040]` low-power guard (`HouseClass::Update
                // 0x004F8B3C..0x004F8DAB`): both live in the native save.
                house.eva_funds_timer.hash(hasher);
                house.eva_low_power_guard.hash(hasher);
            }
        }
    }

    /// Hash all production-related state: queues, ready items, resources.
    fn hash_production(&self, hasher: &mut impl Hasher, schema: HashSchema) {
        let retired_tiberium_fold = !schema.includes(HashFeature::RetiredTiberiumNodeState);
        // P5d: the per-`BuildQueueItem` `queues_by_owner` fold is RETIRED — the
        // queue-of-record now lives in the factory registry (active build = `Factory`
        // head fields; tail = `Factory.queue` of `QueueEntry`) and folds in
        // `hash_factory_registry`. `remaining_base_frames` no longer exists (it was a
        // `BuildQueueItem` field); the sidebar ETA derives it from `progress` at view time
        // and it is intentionally NOT hashed (the 18->19 shape change). `ready_by_owner`,
        // `active_producer_by_owner`, and `next_enqueue_order` are UNCHANGED below.
        for (owner, ready) in &self.production.ready_by_owner {
            owner.hash(hasher);
            for type_id in ready {
                type_id.hash(hasher);
            }
        }
        for (owner, categories) in &self.production.active_producer_by_owner {
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
        // Hash terrain spawners (TIBTRE-style ore generators).
        for (&(rx, ry), spawner) in &self.production.terrain_spawners {
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
        for (&(rx, ry), &bits) in &self.production.terrain_occupation_bits {
            rx.hash(hasher);
            ry.hash(hasher);
            bits.hash(hasher);
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
    /// contract. Explicit-field folding (NOT `#[derive(Hash)]`) so `SpecialItem`'s
    /// three states + the Option presence tags fold distinctly, consistent with the
    /// rest of this file.
    fn hash_factory_registry(&self, hasher: &mut impl Hasher) {
        for f in self.production.factory_shadow.iter_insertion_ordered() {
            f.owner.hash(hasher);
            (f.category as u8).hash(hasher);
            f.insertion_seq.hash(hasher);
            f.progress.hash(hasher);
            f.step_rate_frames.hash(hasher);
            f.step_timer.hash(hasher);
            f.balance.hash(hasher);
            f.original_balance.hash(hasher);
            // P5d: the active build's ETA basis (was the front `BuildQueueItem.total_base_frames`).
            f.active_total_base_frames.hash(hasher);
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
            match f.special {
                crate::sim::production::SpecialItem::NoneNeg1 => 0u8.hash(hasher),
                crate::sim::production::SpecialItem::NoneZero => 1u8.hash(hasher),
                crate::sim::production::SpecialItem::Item(v) => {
                    2u8.hash(hasher);
                    v.hash(hasher);
                }
            }
            // P5d: the queue-of-record (was the per-`BuildQueueItem` `queues_by_owner` fold,
            // now retired). Folds in FIFO (`VecDeque`) order — deterministic by construction.
            (f.queue.len() as u64).hash(hasher);
            for e in &f.queue {
                e.type_id.hash(hasher);
                e.enqueue_order.hash(hasher);
                e.total_base_frames.hash(hasher);
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
            state.power_blackout_remaining.hash(hasher);
            if state.has_drained_power_source {
                b"House.DrainedPowerSource".hash(hasher);
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn state_hash_without_sustained_gap_sight_v142(&self) -> u64 {
        self.state_hash_with_schema(HashSchema::Before(142))
    }

    /// Hash fog-of-war visibility and house alliance data.
    fn hash_fog_and_alliances(&self, hasher: &mut impl Hasher, schema: HashSchema) {
        self.fog.width.hash(hasher);
        self.fog.height.hash(hasher);
        for (owner, fog) in &self.fog.by_owner {
            owner.hash(hasher);
            if schema.includes(HashFeature::SustainedGapSight) {
                fog.cells_raw().hash(hasher);
                fog.shroud_knowledge_raw().hash(hasher);
            } else {
                // Historical probes predate the four persisted provenance bits.
                fog.cells_raw()
                    .iter()
                    .map(|cell| cell & !0xF0)
                    .collect::<Vec<_>>()
                    .hash(hasher);
            }
            // CellClass visibility counters/flags are serialized simulation
            // state, not renderer cache; fold their row-major projection too.
            for cell in fog.cell_runtime_raw() {
                cell.shroud_counter.hash(hasher);
                cell.gap_shroud_counter.hash(hasher);
                cell.alt_flags.hash(hasher);
                cell.flags.hash(hasher);
                cell.visibility.hash(hasher);
                cell.foggedness.hash(hasher);
            }
            fog.visibility_marks_raw().hash(hasher);
        }
        if schema.includes(HashFeature::SustainedGapSight) {
            self.fog.gap_sources.hash(hasher);
            self.fog.sight_admissions.hash(hasher);
            self.fog.whole_map_revealed_owners.hash(hasher);
        }
        b"fogged-object-footprints-v1".hash(hasher);
        self.fog.next_fogged_object_id.hash(hasher);
        self.fog.fogged_object_cells.hash(hasher);
        self.fog.fogged_objects.hash(hasher);
        self.fog.sensors_by_house.hash(hasher);
        if schema.includes(HashFeature::DisguiseDetect) {
            // `CellClass+0xAC[house]` disguise-detect counters.
            // Behaviour-affecting like their `SensorsOfHouses` sibling above:
            // `FUN_004870F0` reads them inside `IsDisguisedTo`, which decides
            // target acquisition and so changes issued commands, not just
            // presentation.
            self.fog.disguise_detect_by_house.hash(hasher);
        }
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
        let mut entries: Vec<_> = bridge_state.iter_cells().collect();
        entries.sort_by_key(|((rx, ry), _)| (*rx, *ry));
        for ((rx, ry), cell) in entries {
            rx.hash(hasher);
            ry.hash(hasher);
            cell.deck_present.hash(hasher);
            cell.damage_state.hash(hasher);
            cell.destroyable.hash(hasher);
            cell.deck_level.hash(hasher);
            cell.bridge_group_id.hash(hasher);
            cell.axis.hash(hasher);
            cell.role.hash(hasher);
            cell.anchor_span_id.hash(hasher);
            cell.overlay_byte.hash(hasher);
            cell.bridgehead_anchor_class.hash(hasher);
        }
        // Hash AnchorSpan registry (Task 7 added this field). BTreeMap iterates
        // in sorted-key order, so iteration is deterministic.
        for (id, span) in bridge_state.anchor_spans() {
            id.hash(hasher);
            span.hash(hasher);
        }
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
            record.group_id.hash(hasher);
            record.active.hash(hasher);
            record.bridge_kind.hash(hasher);
        }
    }

    fn hash_overlay_grid(&self, hasher: &mut impl Hasher, schema: HashSchema) {
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
        if schema.includes(HashFeature::WallRuntime) {
            b"retained-wall-neighbor-counts-v1".hash(hasher);
            match overlay_grid.retained_neighbor_counts() {
                None => 0u8.hash(hasher),
                Some(counts) => {
                    1u8.hash(hasher);
                    counts.len().hash(hasher);
                    for count in counts {
                        count.hash(hasher);
                    }
                }
            }
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

    /// Hash all occupied smudge cells in stable cell-coord order.
    /// Must be deterministic across replays — visual divergence between clients
    /// is jarring even though smudges are cosmetic.
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
            site.level_timer_start.hash(hasher);
            site.level_timer_duration.hash(hasher);
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
            }
        }
        // Hash lightning storm global state.
        self.lightning_storm.is_some().hash(hasher);
        if let Some(ref ls) = self.lightning_storm {
            ls.owner.hash(hasher);
            ls.target_rx.hash(hasher);
            ls.target_ry.hash(hasher);
            ls.deferment_remaining.hash(hasher);
            ls.duration_remaining.hash(hasher);
            ls.center_bolt_timer.hash(hasher);
            ls.scatter_bolt_timer.hash(hasher);
            ls.last_bolt_rx.hash(hasher);
            ls.last_bolt_ry.hash(hasher);
        }
    }

    /// Hash all entity components in stable-entity-ID order.
    /// BTreeMap iterates in key order (= stable_id), so no manual sort needed.
    fn hash_entities(&self, hasher: &mut impl Hasher, schema: HashSchema) {
        for entity in self.substrate.entities.values() {
            entity.stable_id().hash(hasher);
            if schema.includes(HashFeature::AircraftDockState) {
                // Both mission and legacy ammo FSMs still execute. Hash their
                // actual saved state until their ownership migration retires one.
                if let Some(ammo) = entity.aircraft_ammo.as_ref() {
                    b"aircraft-ammo-v170".hash(hasher);
                    #[cfg(test)]
                    if !schema.includes(HashFeature::AircraftReleaseAuthority) {
                        ammo.hash_before_pending_release(hasher);
                    } else {
                        ammo.hash(hasher);
                    }
                    #[cfg(not(test))]
                    ammo.hash(hasher);
                }
                if let Some(mission) = entity.aircraft_mission.as_ref() {
                    b"aircraft-mission-v170".hash(hasher);
                    mission.hash(hasher);
                    #[cfg(test)]
                    if !schema.includes(HashFeature::AircraftReleaseAuthority)
                        && matches!(
                            mission,
                            crate::sim::aircraft::AircraftMission::Attack { .. }
                        )
                    {
                        // Only the false/false historical fixture is recoverable.
                        false.hash(hasher);
                        false.hash(hasher);
                    }
                }
            }
            if schema.includes(HashFeature::CreditIncome) {
                // GSI-09.01: the `BuildingClass+0x6D0/+0x6D8` ProduceCash
                // timer and the `TechnoClass+0x1CC/+0x1D0` drain link pair.
                // All three drive future wallet writes, so a divergence here
                // desyncs credits; every object carries them.
                entity.produce_cash_timer.hash(hasher);
                entity.drain_target.hash(hasher);
                entity.draining_me.hash(hasher);
            }
            if schema.includes(HashFeature::Parasite) {
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
            }
            if schema.includes(HashFeature::CrewSurvival) && entity.has_been_captured {
                // Building+6E3: doubles the survivor divisor, widens the
                // survivor roll and skips the engineer roll at death.
                0x6e3_u32.hash(hasher);
            }
            if schema.includes(HashFeature::TechnoConstructor)
                && (entity.techno_ctor_random_word != 0 || entity.structure_upgrade_link.is_some())
            {
                b"techno-constructor-v1".hash(hasher);
                entity.techno_ctor_random_word.hash(hasher);
                entity.structure_upgrade_link.hash(hasher);
            }
            if schema.includes(HashFeature::TechnoConstructor) {
                // Constructor-owned SlaveManager identities live in the
                // transitional ProductionState registry until the manager is
                // promoted to its own component. Both the ordered pool and
                // each child's active harvest cursor affect future simulation.
                if let Some(slave_ids) = self.production.slave_bindings.get(&entity.stable_id()) {
                    b"constructor-slave-pool-v1".hash(hasher);
                    slave_ids.len().hash(hasher);
                    for slave_id in slave_ids {
                        slave_id.hash(hasher);
                    }
                }
                if let Some(slave) = entity.slave_harvester.as_ref() {
                    b"slave-harvester-v1".hash(hasher);
                    slave.master_id.hash(hasher);
                    (slave.state as u8).hash(hasher);
                    slave.cargo.len().hash(hasher);
                    for bale in &slave.cargo {
                        (bale.resource_type as u8).hash(hasher);
                        bale.value.hash(hasher);
                    }
                    slave.capacity.hash(hasher);
                    slave.harvest_timer.hash(hasher);
                    slave.target_cell.hash(hasher);
                }
            }
            entity.occupancy_enter_order.hash(hasher);
            entity.air_spatial_bucket.hash(hasher);
            entity.air_spatial_enter_order.hash(hasher);
            if schema.includes(HashFeature::Lifecycle) {
                // Independent lifecycle axes and deterministic Rust bookkeeping.
                // Keep this order fixed: it is part of the lockstep hash contract.
                entity.lifecycle.object_alive.hash(hasher);
                entity.lifecycle.in_limbo.hash(hasher);
                entity.lifecycle.cell_marked.hash(hasher);
                entity.dying.hash(hasher);
                entity.dirty_rect_eligible.hash(hasher);
                entity.owned_count_released.hash(hasher);
            }
            if schema.includes(HashFeature::InfantryTerminal) {
                entity.infantry_terminal.hash(hasher);
            }
            if schema.includes(HashFeature::TechnoPlayfield) {
                // TechnoClass+0x3D5 is mutable admission state, not a derived
                // position query: ordinary movement is promote-only while
                // teleport and Set_Clipped_LocalSize own exact demotions.
                entity.in_playfield.hash(hasher);
            }
            entity.move_sound_active.hash(hasher);
            if schema.includes(HashFeature::TechnoMissionOnly) {
                entity.is_mission_only().hash(hasher);
            }
            entity.move_sound_countdown.hash(hasher);
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
            entity.facing.hash(hasher);
            entity.facing_target.hash(hasher);
            // Body-rotation interpolator (present only while turning in place).
            if let Some(ref bf) = entity.body_facing {
                1u8.hash(hasher);
                bf.hash(hasher);
            } else {
                0u8.hash(hasher);
            }
            entity.body_frame_counter.hash(hasher);
            // Building+6E6 is retained transition state, independent of HP.
            if entity.building_damage_state_active {
                b"building-damage-state-v1".hash(hasher);
            }
            if entity.building_actually_placed
                || entity.building_anim_slots.iter().any(Option::is_some)
                || (schema.includes_building_power_integration()
                    && entity.building_anim_effect_replay.iter().any(|v| *v))
            {
                b"building-anim-slots-v1".hash(hasher);
                entity.building_actually_placed.hash(hasher);
                entity.building_anim_slots.hash(hasher);
                if schema.includes_building_power_integration() {
                    entity.building_anim_effect_replay.hash(hasher);
                }
            }
            if entity.building_storage != Default::default() {
                b"building-storage-v1".hash(hasher);
                entity.building_storage.hash(hasher);
            }
            if schema.includes_building_power_integration() && entity.building_last_operational {
                b"building-operational-v1".hash(hasher);
            }
            if schema.includes_building_power_integration() && !entity.building_stuff_enabled {
                b"building-stuff-disabled-v1".hash(hasher);
            }
            if schema.includes(HashFeature::AnimationAuthority) && entity.building_has_engineer {
                b"building-has-engineer-v1".hash(hasher);
            }
            if schema.includes(HashFeature::EntityAnimation)
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
            if schema.includes(HashFeature::EstimatedHealth) {
                entity.estimated_health.get().hash(hasher);
            }
            entity.type_ref().hash(hasher);
            (entity.category as u8).hash(hasher);
            entity.foundation.hash(hasher);
            entity.building_hidden_occupancy.hash(hasher);
            entity.base_reservation_spacing.hash(hasher);
            entity.determines_waypoint_edge.hash(hasher);
            if schema.includes(HashFeature::NavalBuildConst) && entity.build_const_eligible {
                b"naval-build-const-entity-v1".hash(hasher);
                entity.build_const_eligible.hash(hasher);
            }
            if schema.includes(HashFeature::BasePlan) {
                entity.base_plan_type_index.hash(hasher);
                entity.base_plan_is_defense.hash(hasher);
                entity.base_plan_has_undeploy_target.hash(hasher);
            }
            entity.veterancy.hash(hasher);
            // The raw accumulator is authoritative — `veterancy` is only its
            // rank projection, so two objects one kill apart inside the same
            // rank are distinct sim state.
            entity.veterancy_raw.bits().hash(hasher);
            entity.veterancy_rank_cache.hash(hasher);
            // Folded only while armed so the legacy default-zero hash stream
            // is preserved (the `no_damage` pattern).
            if entity.elite_flash_frames != 0 {
                b"elite-flash-v1".hash(hasher);
                entity.elite_flash_frames.hash(hasher);
            }
            entity.armor_multiplier.bits().hash(hasher);
            entity.berserk.hash(hasher);
            entity.was_attacked_by_enemy.hash(hasher);
            if schema.includes(HashFeature::BaseDefenseResponse) {
                b"base-defense-response-v1".hash(hasher);
                entity.base_defense_response.hash(hasher);
            }
            entity.regular_crusher.hash(hasher);
            entity.drive_accelerates.hash(hasher);
            entity.damage_fire_state_active.hash(hasher);
            entity.damage_fire_anim_ids.hash(hasher);
            entity.vision_range.hash(hasher);
            if schema.includes(HashFeature::CellMembership) {
                entity.sight_is_zero.hash(hasher);
            }
            if schema.includes(HashFeature::SustainedGapSight) {
                entity.sight_refresh_timers.hash(hasher);
            }
            if schema.includes(HashFeature::GapOperational)
                && entity.gap_generator != crate::sim::vision::GapGeneratorRuntime::default()
            {
                b"gap-operational-v144".hash(hasher);
                entity.gap_generator.hash(hasher);
            }

            if let Some(ref movement) = entity.movement_target {
                1u8.hash(hasher);
                movement.next_index.hash(hasher);
                movement.speed.hash(hasher);
                if !schema.includes(HashFeature::FootPathRuntime) {
                    // Historical positional encoding of the former adapter
                    // fields. This projection is not native timer equality.
                    let path = &entity.navigation.path_runtime;
                    (path
                        .movement_timer
                        .remaining(self.session.binary_frame as i32) as u16)
                        .hash(hasher);
                    (path
                        .blocked_timer
                        .remaining(self.session.binary_frame as i32) as u16)
                        .hash(hasher);
                    path.path_blocked.hash(hasher);
                    (path.retries_left as u8).hash(hasher);
                }
                movement.path.hash(hasher);
                movement.path_layers.hash(hasher);
            } else {
                0u8.hash(hasher);
            }

            if schema.includes(HashFeature::FootPathRuntime) {
                entity.navigation.path_runtime.hash(hasher);
            }
            if schema.includes(HashFeature::FootNeighborHistory) {
                entity.navigation.neighbor_state.hash(hasher);
            }
            entity.navigation.path_replay.hash(hasher);
            entity.navigation.nav_com_aux.hash(hasher);
            entity.navigation.nav_com.hash(hasher);
            entity.navigation.suspended_nav_com.hash(hasher);
            entity.navigation.nav_queue.hash(hasher);
            entity.navigation.pending_arrival_clear.hash(hasher);

            #[cfg(test)]
            if !schema.includes(HashFeature::TrackAuthority) {
                assert_retired_track_projection_is_bounded(entity);
                // Schema166's entity.drive_track == None tag, before the
                // retained classes. The original payload is not recoverable.
                0u8.hash(hasher);
            }
            hash_retained_track_classes(entity, schema, hasher);
            entity.foot_speed.applied_fraction.hash(hasher);
            entity.foot_speed.cached_current_speed.hash(hasher);
            if schema.includes(HashFeature::FlyLanding)
                && entity.flight_attitude != Default::default()
            {
                0x2e8_u32.hash(hasher);
                entity.flight_attitude.hash(hasher);
            }
            if schema.includes(HashFeature::FootCrateSpeed) {
                entity.foot_speed.crate_multiplier().hash(hasher);
            }
            entity.foot_occupation_enabled.hash(hasher);
            entity.foot_locomotor_swap_active.hash(hasher);

            #[cfg(test)]
            if !schema.includes(HashFeature::TrackAuthority) {
                // Schema166's entity.forced_drive_track == None tag follows
                // owner speed/occupation. Current production folds neither tag.
                0u8.hash(hasher);
            }
            if let Some(ref loco) = entity.locomotor {
                1u8.hash(hasher);
                (loco.kind as u8).hash(hasher);
                // The installed slot, distinct from the active kind: the two
                // differ while a piggyback stash is up, so hashing only the
                // active class would let a desync in which locomotor a unit was
                // built with hide behind a matching current one.
                loco.slot.installed().hash(hasher);
                // Locomotor power: deterministic state with an observable Hover
                // effect, and deploy/undeploy flips it, so it must be in the
                // lockstep hash.
                loco.powered.hash(hasher);
                (loco.layer as u8).hash(hasher);
                (loco.phase as u8).hash(hasher);
                // Hover throttle is authoritative movement state (persists across
                // repaths); I16F16 has no Hash — fold the raw bits. The vertical
                // pair (altitude + bob spring state) is likewise authoritative:
                // altitude feeds combat's effective-Z and the hover float.
                loco.hover_throttle.to_bits().hash(hasher);
                loco.hover_bob_offset.to_bits().hash(hasher);
                loco.altitude.to_bits().hash(hasher);
                hash_locomotor_payload(&loco.runtime_payload, hasher, schema);
                match loco.piggyback.as_deref() {
                    Some(runtime) => {
                        1u8.hash(hasher);
                        hash_locomotor_runtime(runtime, hasher, schema);
                    }
                    None => 0u8.hash(hasher),
                }
                // Mission readiness inputs are NOT hashed: they are derived at
                // the gate from state already hashed above (and from position,
                // facing and movement target, likewise hashed). Hashing a
                // derived predicate would pin a projection of the same state
                // twice, and it cannot diverge independently of its inputs.
            } else {
                0u8.hash(hasher);
            }

            if let Some(ref bridge) = entity.bridge_occupancy {
                1u8.hash(hasher);
                bridge.deck_level.hash(hasher);
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
            hash_teleport_state(entity.teleport_state.as_ref(), hasher);
            hash_tunnel_state(entity.tunnel_state.as_ref(), hasher);
            hash_rocket_state(entity.rocket_state.as_ref(), hasher);
            hash_drop_pod_state(entity.drop_pod_state.as_ref(), hasher);
            if let Some(cloak) = entity.cloak.as_ref() {
                1u8.hash(hasher);
                cloak.state.hash(hasher);
                cloak.visual_phase.map(|phase| phase as u8).hash(hasher);
                cloak.depth.hash(hasher);
                cloak.cloaking_stages.hash(hasher);
                cloak.late_visible.hash(hasher);
                cloak.force_visible_call.hash(hasher);
                cloak.step_delta.hash(hasher);
                cloak.step_timer.start_frame.hash(hasher);
                cloak.step_timer.speed.hash(hasher);
                cloak.step_timer.duration_frames.hash(hasher);
                cloak.recloak_delay_start.hash(hasher);
                cloak.recloak_delay_frames.hash(hasher);
                cloak.secondary_gate_start.hash(hasher);
                cloak.secondary_gate_frames.hash(hasher);
            } else {
                0u8.hash(hasher);
            }
            if schema.includes(HashFeature::SensorDeposit) {
                if let Some(deposit) = entity.sensor_deposit {
                    1u8.hash(hasher);
                    deposit.owner.hash(hasher);
                    deposit.center.hash(hasher);
                    deposit.add_radius.hash(hasher);
                    deposit.remove_radius.hash(hasher);
                    deposit.building_array.hash(hasher);
                    if schema.includes(HashFeature::DisguiseDetect) {
                        // The cached `DetectDisguiseRange=` circle. It is the
                        // radius Limbo will decrement, so it selects which
                        // cells leave the disguise-detect plane.
                        deposit.detect_disguise_radius.hash(hasher);
                    }
                } else {
                    0u8.hash(hasher);
                }
            }
            if let Some(disguise) = entity.disguise.as_ref() {
                1u8.hash(hasher);
                disguise.disguised.hash(hasher);
                disguise.disguise_creation_frame.hash(hasher);
                disguise.disguise_type.hash(hasher);
                disguise.disguised_as_house.hash(hasher);
                disguise.reveal.start_frame.hash(hasher);
                disguise.reveal.neighbor_cell_packed.hash(hasher);
                disguise.reveal.duration_frames.hash(hasher);
            } else {
                0u8.hash(hasher);
            }

            if let Some(ref inv) = entity.invulnerability {
                1u8.hash(hasher);
                inv.start_frame.hash(hasher);
                inv.duration_frames.hash(hasher);
                let kind_byte: u8 = match inv.kind {
                    crate::sim::superweapon::invulnerability::InvulnKind::IronCurtain => 0,
                    crate::sim::superweapon::invulnerability::InvulnKind::ForceShield => 1,
                };
                kind_byte.hash(hasher);
            } else {
                0u8.hash(hasher);
            }

            if let Some(ref attack) = entity.attack_target {
                1u8.hash(hasher);
                attack.cooldown_ticks.hash(hasher);
                attack.target.hash(hasher);
                if !schema.includes(HashFeature::WeaponBurstAuthority) {
                    // Bounded historical replay fixtures had no remaining
                    // burst shots. Arbitrary old AttackTarget state is unrecoverable.
                    0u8.hash(hasher);
                }
                attack.burst_delay_ticks.hash(hasher);
                attack.pending_infantry_fire.hash(hasher);
            } else {
                0u8.hash(hasher);
            }
            if schema.includes(HashFeature::WeaponBurstAuthority) {
                entity.weapon_burst.hash(hasher);
            }
            entity.pending_building_fire.hash(hasher);
            entity.current_weapon_index.hash(hasher);
            entity.current_weapon_ref.map(|id| id.index()).hash(hasher);

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
            entity.rally_target.hash(hasher);
            entity.capture_target.hash(hasher);
            entity.c4_plant.hash(hasher);
            match entity.pending_c4_detonation {
                Some(pending) => {
                    true.hash(hasher);
                    pending
                        .remaining_at(self.session.binary_frame as i32)
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
                gate.mission_18_active.hash(hasher);
                (gate.phase as u8).hash(hasher);
                (gate.mission_state as u8).hash(hasher);
                // Same u32 values, same order as the old (last_frame, ticks_remaining)
                // pairs — the MissionTimer regrouping leaves the hash pre-image identical.
                gate.transition_timer.duration.hash(hasher);
                gate.transition_total_ticks.hash(hasher);
                gate.transition_timer.start_frame.hash(hasher);
                gate.hold_timer.duration.hash(hasher);
                gate.hold_timer.start_frame.hash(hasher);
            } else {
                0u8.hash(hasher);
            }

            // Unit+68C remains the deployment authority. The former MCV-only
            // turn observation now hashes once in its retained locomotor class.
            if entity.mcv_deploy_pending {
                0x4d435644u32.hash(hasher);
                entity.mcv_deploy_pending.hash(hasher);
                #[cfg(test)]
                if !schema.includes(HashFeature::TrackAuthority) {
                    // The bounded historical projection above rejects retained
                    // turn/track state; only its established false latch projects.
                    false.hash(hasher);
                }
            }
            match entity.deploy_state {
                None => 0u8.hash(hasher),
                Some(crate::sim::deploy::DeployPhase::Deploying { ticks_remaining }) => {
                    1u8.hash(hasher);
                    ticks_remaining.hash(hasher);
                }
                Some(crate::sim::deploy::DeployPhase::Deployed) => {
                    2u8.hash(hasher);
                }
                Some(crate::sim::deploy::DeployPhase::Undeploying { ticks_remaining }) => {
                    3u8.hash(hasher);
                    ticks_remaining.hash(hasher);
                }
            }

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
                miner.home_refinery.hash(hasher);
                miner.reserved_refinery.hash(hasher);
                miner.target_ore_cell.hash(hasher);
                // harvest_timer is now a MissionTimer (start_frame + duration)
                // — intended one-time re-baseline. unload_timer was deleted.
                miner.harvest_timer.hash(hasher);
                miner.forced_return.hash(hasher);
                miner.dock_queued.hash(hasher);
                miner.dock_phase.hash(hasher);
                miner.dock_pivot_facing.hash(hasher);
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
                    phase,
                } => {
                    2u8.hash(hasher);
                    target_transport_id.hash(hasher);
                    (*phase as u8).hash(hasher);
                }
                crate::sim::passenger::PassengerRole::Inside { transport_id } => {
                    3u8.hash(hasher);
                    transport_id.hash(hasher);
                }
            }
            entity.weapon_override.hash(hasher);
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
                if schema.includes(HashFeature::MindControl) {
                    manager.hash(hasher);
                } else {
                    manager.hash_before_mind_control(hasher);
                }
            }
            if schema.includes(HashFeature::MindControl)
                && entity.mind_control != crate::sim::capture_manager::MindControlLink::default()
            {
                // The victim's MindControlledBy (+2C0) and ring (+2C8).
                0x2c0_u32.hash(hasher);
                entity.mind_control.hash(hasher);
            }
            // Homing missile flight state. `HomingState` has a manual `Hash`
            // impl that excludes the render-only `pitch: f32` field — see
            // sim::movement::homing_movement.
            if let Some(ref h) = entity.homing_state {
                1u8.hash(hasher);
                h.hash(hasher);
            } else {
                0u8.hash(hasher);
            }
            // Barrel facing — Hash-derived, all primitive fields contribute.
            if let Some(ref barrel) = entity.barrel_facing {
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

            if schema.includes(HashFeature::Mission) {
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
                match entity.suspended_attack_target {
                    Some(target) => {
                        1u8.hash(hasher);
                        target.hash(hasher);
                    }
                    None => 0u8.hash(hasher),
                }
                entity.object_is_falling_down.hash(hasher);
            } else {
                hash_mission_com_before_v29(&entity.mission, hasher);
            }
            if !schema.includes(HashFeature::AircraftReleaseAuthority) {
                // Bounded historical fixtures had no fabricated release tail.
                // An arbitrary old active tail cannot be reconstructed.
                0u8.hash(hasher);
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
    fn hash_anims(&self, hasher: &mut impl Hasher, _schema: HashSchema) {
        self.substrate.anims.iter().count().hash(hasher);
        for (id, anim) in self.substrate.anims.iter() {
            id.hash(hasher);
            #[cfg(test)]
            if !_schema.includes(HashFeature::AnimationDisplay) {
                anim.hash_before_display(hasher);
                continue;
            }
            anim.hash(hasher);
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
            let bounce = &debris.bounce;
            bounce.elasticity.bits().hash(hasher);
            bounce.gravity.bits().hash(hasher);
            bounce.angular_velocity_magnitude.bits().hash(hasher);
            for axis in 0..3 {
                bounce.position[axis].bits().hash(hasher);
                bounce.velocity[axis].bits().hash(hasher);
                bounce.spin_axis[axis].bits().hash(hasher);
            }
            bounce.spin_angle.bits().hash(hasher);
        }
    }
}

fn hash_locomotor_runtime(
    runtime: &crate::sim::movement::locomotion::piggyback::LocomotorRuntime,
    hasher: &mut impl Hasher,
    schema: HashSchema,
) {
    (runtime.kind as u8).hash(hasher);
    (runtime.layer as u8).hash(hasher);
    let common = &runtime.common;
    common.powered.hash(hasher);
    (common.phase as u8).hash(hasher);
    // Fixed separators retain the retired common-air slots for non-air replay
    // stability. Fly/Jumpjet authoritative fields are hashed in their payloads.
    0u8.hash(hasher);
    common.speed_multiplier.to_bits().hash(hasher);
    common.speed_fraction.to_bits().hash(hasher);
    common.fly_current_speed.to_bits().hash(hasher);
    common.altitude.to_bits().hash(hasher);
    0i32.hash(hasher);
    0i32.hash(hasher);
    common.jumpjet_speed.to_bits().hash(hasher);
    common.jumpjet_accel.to_bits().hash(hasher);
    common.jumpjet_current_speed.to_bits().hash(hasher);
    common.jumpjet_deviation.hash(hasher);
    common.jumpjet_crash_speed.to_bits().hash(hasher);
    common.jumpjet_turn_rate.hash(hasher);
    common.balloon_hover.hash(hasher);
    common.hover_attack.hash(hasher);
    common.speed_type.hash(hasher);
    common.movement_zone.hash(hasher);
    common.rot.hash(hasher);
    common.air_progress.to_bits().hash(hasher);
    common.infantry_wobble_phase.to_bits().hash(hasher);
    common
        .subcell_dest
        .map(|(x, y)| (x.to_bits(), y.to_bits()))
        .hash(hasher);
    common.hover_throttle.to_bits().hash(hasher);
    common.hover_speed_request.to_bits().hash(hasher);
    common.hover_bob_offset.to_bits().hash(hasher);
    hash_locomotor_payload(&runtime.payload, hasher, schema);
}

fn hash_locomotor_payload(
    payload: &crate::sim::movement::locomotion::piggyback::LocomotorRuntimePayload,
    hasher: &mut impl Hasher,
    schema: HashSchema,
) {
    use crate::sim::movement::locomotion::piggyback::LocomotorRuntimePayload;
    match payload {
        LocomotorRuntimePayload::Drive(state) => {
            0u8.hash(hasher);
            hash_slope_transition_state(state, hasher);
        }
        LocomotorRuntimePayload::Walk(state) => {
            1u8.hash(hasher);
            if schema.includes(HashFeature::BridgeLocomotorAndDummy) {
                state.hash(hasher);
            }
        }
        LocomotorRuntimePayload::Teleport(state) => {
            2u8.hash(hasher);
            hash_teleport_state(state.as_ref(), hasher);
        }
        LocomotorRuntimePayload::Tunnel(state) => {
            3u8.hash(hasher);
            hash_tunnel_state(state.as_ref(), hasher);
        }
        LocomotorRuntimePayload::Rocket(state) => {
            4u8.hash(hasher);
            hash_rocket_state(state.as_ref(), hasher);
        }
        LocomotorRuntimePayload::DropPod(state) => {
            5u8.hash(hasher);
            hash_drop_pod_state(state.as_ref(), hasher);
        }
        LocomotorRuntimePayload::Hover(head) => {
            6u8.hash(hasher);
            if schema.includes(HashFeature::BridgeLocomotorAndDummy) {
                head.hash(hasher);
            }
        }
        LocomotorRuntimePayload::Mech => 7u8.hash(hasher),
        LocomotorRuntimePayload::Ship(state) => {
            8u8.hash(hasher);
            hash_slope_transition_state(state, hasher);
        }
        LocomotorRuntimePayload::Fly(state) => {
            9u8.hash(hasher);
            let (target_height, taking_off, landing) = state.height_hash_fields();
            target_height.hash(hasher);
            taking_off.hash(hasher);
            landing.hash(hasher);
            if schema.includes(HashFeature::FlyDestination) {
                state.destination().hash(hasher);
            }
            if schema.includes(HashFeature::FlyCruiseMode) {
                state.cruise_mode().hash(hasher);
            }
            if schema.includes(HashFeature::FlyLanding) {
                state.moving().hash(hasher);
                state.landing_effect_latched().hash(hasher);
                state.airport_bound().hash(hasher);
            }
        }
        LocomotorRuntimePayload::Jumpjet(state) => {
            10u8.hash(hasher);
            if schema.includes(HashFeature::BridgeLocomotorAndDummy) {
                state.hash(hasher);
            }
        }
        LocomotorRuntimePayload::Parachute => 11u8.hash(hasher),
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

/// TeleportLocomotionClass::Process @ 0x007192f0 owns this complete, named
/// runtime state; its target and materialization timer must not escape lockstep.
fn hash_teleport_state(
    state: Option<&crate::sim::movement::teleport_movement::TeleportState>,
    hasher: &mut impl Hasher,
) {
    match state {
        None => 0u8.hash(hasher),
        Some(state) => {
            1u8.hash(hasher);
            let phase = match state.phase {
                crate::sim::movement::teleport_movement::TeleportPhase::Relocate => 0u8,
                crate::sim::movement::teleport_movement::TeleportPhase::ChronoDelay => 1,
            };
            phase.hash(hasher);
            state.target_rx.hash(hasher);
            state.target_ry.hash(hasher);
            state.being_warped_ticks.hash(hasher);
        }
    }
}

/// YR TunnelLocomotionClass keeps the phase byte in its locomotor runtime.
/// Keep the projection explicit instead of relying on a Rust derived hash.
fn hash_tunnel_state(
    state: Option<&crate::sim::movement::tunnel_movement::TunnelState>,
    hasher: &mut impl Hasher,
) {
    match state {
        None => 0u8.hash(hasher),
        Some(state) => {
            1u8.hash(hasher);
            (state.phase as u8).hash(hasher);
        }
    }
}

/// RocketLocomotionClass::Process @ 0x006622c0 owns the complete flight table
/// selection and current flight state. `pitch` is render-only, so it is omitted.
fn hash_rocket_state(
    state: Option<&crate::sim::movement::rocket_movement::RocketState>,
    hasher: &mut impl Hasher,
) {
    match state {
        None => 0u8.hash(hasher),
        Some(state) => {
            1u8.hash(hasher);
            let phase = match state.phase {
                crate::sim::movement::rocket_movement::RocketPhase::Ignition => 0u8,
                crate::sim::movement::rocket_movement::RocketPhase::Tilt => 1,
                crate::sim::movement::rocket_movement::RocketPhase::Ascent => 2,
                crate::sim::movement::rocket_movement::RocketPhase::Cruise => 3,
                crate::sim::movement::rocket_movement::RocketPhase::Terminal => 4,
                crate::sim::movement::rocket_movement::RocketPhase::Secondary => 5,
            };
            phase.hash(hasher);
            state.origin_rx.hash(hasher);
            state.origin_ry.hash(hasher);
            state.target_rx.hash(hasher);
            state.target_ry.hash(hasher);
            state.speed.to_bits().hash(hasher);
            state.current_speed.to_bits().hash(hasher);
            state.altitude.to_bits().hash(hasher);
            state.progress.to_bits().hash(hasher);
            state.phase_frames.hash(hasher);
            state.parameters.acceleration.to_bits().hash(hasher);
            state.parameters.max_speed.to_bits().hash(hasher);
            state.parameters.ascent_altitude.to_bits().hash(hasher);
            state.parameters.tilt_rate.to_bits().hash(hasher);
            state.parameters.relaunches.hash(hasher);
        }
    }
}

/// DropPodLocomotionClass flight state is lockstep state even while it has no
/// surface occupation. This mirrors the typed serialized runtime exactly.
fn hash_drop_pod_state(
    state: Option<&crate::sim::movement::drop_pod_movement::DropPodState>,
    hasher: &mut impl Hasher,
) {
    match state {
        None => 0u8.hash(hasher),
        Some(state) => {
            1u8.hash(hasher);
            let phase = match state.phase {
                crate::sim::movement::drop_pod_movement::DropPodPhase::Descending => 0u8,
                crate::sim::movement::drop_pod_movement::DropPodPhase::Landed => 1,
                crate::sim::movement::drop_pod_movement::DropPodPhase::Destroyed => 2,
            };
            phase.hash(hasher);
            state.target_rx.hash(hasher);
            state.target_ry.hash(hasher);
            state.altitude.to_bits().hash(hasher);
            state.descent_speed.to_bits().hash(hasher);
            state.elapsed_frames.hash(hasher);
        }
    }
}

#[cfg(test)]
mod teleport_rocket_hash_tests {
    use super::Simulation;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::movement::rocket_movement::{RocketFlightParameters, RocketPhase, RocketState};
    use crate::sim::movement::teleport_movement::{TeleportPhase, TeleportState};
    use crate::util::fixed_math::SimFixed;

    fn teleport_state() -> TeleportState {
        TeleportState {
            phase: TeleportPhase::Relocate,
            target_rx: 17,
            target_ry: 29,
            being_warped_ticks: 41,
        }
    }

    fn rocket_state() -> RocketState {
        RocketState {
            phase: RocketPhase::Cruise,
            origin_rx: 3,
            origin_ry: 5,
            target_rx: 17,
            target_ry: 29,
            speed: SimFixed::from_num(11),
            current_speed: SimFixed::from_num(7),
            altitude: SimFixed::from_num(400),
            progress: SimFixed::from_num(0.5),
            phase_frames: 13,
            parameters: RocketFlightParameters {
                acceleration: SimFixed::from_num(90),
                max_speed: SimFixed::from_num(11),
                ascent_altitude: SimFixed::from_num(400),
                tilt_rate: SimFixed::from_num(0.35),
                relaunches: 2,
            },
            pitch: 0.25,
            payload: None,
        }
    }

    fn hash_entity(mut entity: GameEntity) -> u64 {
        let mut sim = Simulation::new();
        entity.stable_id = 1;
        sim.substrate.entities.insert(entity);
        sim.state_hash()
    }

    fn hash_teleport(state: Option<TeleportState>) -> u64 {
        let mut entity = GameEntity::test_default(1, "CHRP", "Americans", 5, 5);
        entity.teleport_state = state;
        hash_entity(entity)
    }

    fn hash_rocket(state: Option<RocketState>) -> u64 {
        let mut entity = GameEntity::test_default(1, "V3RKT", "Soviet", 5, 5);
        entity.rocket_state = state;
        hash_entity(entity)
    }

    fn assert_teleport_change(change: impl FnOnce(&mut TeleportState)) {
        let baseline = hash_teleport(Some(teleport_state()));
        let mut changed = teleport_state();
        change(&mut changed);
        assert_ne!(baseline, hash_teleport(Some(changed)));
    }

    fn assert_rocket_change(change: impl FnOnce(&mut RocketState)) {
        let baseline = hash_rocket(Some(rocket_state()));
        let mut changed = rocket_state();
        change(&mut changed);
        assert_ne!(baseline, hash_rocket(Some(changed)));
    }

    #[test]
    fn teleport_hash_projects_presence_phase_target_and_materialization_timer() {
        assert_ne!(hash_teleport(None), hash_teleport(Some(teleport_state())));
        assert_teleport_change(|state| state.phase = TeleportPhase::ChronoDelay);
        assert_teleport_change(|state| state.target_rx += 1);
        assert_teleport_change(|state| state.target_ry += 1);
        assert_teleport_change(|state| state.being_warped_ticks += 1);
    }

    #[test]
    fn rocket_hash_projects_complete_simulation_flight_runtime() {
        assert_ne!(hash_rocket(None), hash_rocket(Some(rocket_state())));
        assert_rocket_change(|state| state.phase = RocketPhase::Terminal);
        assert_rocket_change(|state| state.origin_rx += 1);
        assert_rocket_change(|state| state.origin_ry += 1);
        assert_rocket_change(|state| state.target_rx += 1);
        assert_rocket_change(|state| state.target_ry += 1);
        assert_rocket_change(|state| state.speed += SimFixed::from_num(1));
        assert_rocket_change(|state| state.current_speed += SimFixed::from_num(1));
        assert_rocket_change(|state| state.altitude += SimFixed::from_num(1));
        assert_rocket_change(|state| state.progress += SimFixed::from_num(0.1));
        assert_rocket_change(|state| state.phase_frames += 1);
        assert_rocket_change(|state| state.parameters.acceleration += SimFixed::from_num(1));
        assert_rocket_change(|state| state.parameters.max_speed += SimFixed::from_num(1));
        assert_rocket_change(|state| state.parameters.ascent_altitude += SimFixed::from_num(1));
        assert_rocket_change(|state| state.parameters.tilt_rate += SimFixed::from_num(0.1));
        assert_rocket_change(|state| state.parameters.relaunches += 1);
    }

    #[test]
    fn rocket_hash_excludes_explicit_render_only_pitch() {
        let baseline = hash_rocket(Some(rocket_state()));
        let mut render_only_change = rocket_state();
        render_only_change.pitch = 0.75;
        assert_eq!(baseline, hash_rocket(Some(render_only_change)));
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
    use crate::map::authored_overlay::FinalizedOverlayPayload;
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
            FinalizedOverlayPayload::from_cells_for_test(2, 2, cells.clone(), vec![0, 1, 0, 0]),
        ));
        sim_b.overlay_grid = Some(OverlayGrid::from_finalized_map_payload(
            FinalizedOverlayPayload::from_cells_for_test(2, 2, cells, vec![0, 2, 0, 0]),
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
        assert_eq!(
            sim_a.state_hash_without_wall_runtime_v115(),
            sim_b.state_hash_without_wall_runtime_v115(),
            "the v114 provenance schema ended after final overlay-cell state"
        );
    }

    #[test]
    fn retained_wall_neighbor_authority_mode_changes_hash_even_when_zero() {
        let mut legacy = Simulation::new();
        legacy.overlay_grid = Some(OverlayGrid::new(2, 2));

        let mut retained = Simulation::new();
        retained.overlay_grid = Some(OverlayGrid::from_finalized_map_payload(
            FinalizedOverlayPayload::from_cells_for_test(2, 2, vec![(-1, 0); 4], vec![0; 4]),
        ));

        for y in 0..2 {
            for x in 0..2 {
                assert_eq!(
                    legacy.overlay_grid.as_ref().expect("legacy").cell(x, y),
                    retained.overlay_grid.as_ref().expect("retained").cell(x, y)
                );
            }
        }
        assert_ne!(
            legacy.state_hash(),
            retained.state_hash(),
            "None and Some(all-zero) select different future wall-count behavior"
        );
        assert_eq!(
            legacy.state_hash_without_wall_runtime_v115(),
            retained.state_hash_without_wall_runtime_v115(),
            "retained authority mode begins with schema v115"
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
            entity.owned_count_released = !entity.owned_count_released;
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
        for (field, variant) in [
            (
                "firing sequence latch",
                MissionLeafState::infantry_raw_for_test(8, 12),
            ),
            ("Doing", MissionLeafState::infantry_raw_for_test(7, 13)),
        ] {
            assert_ne!(
                base_hash,
                hash_leaf(variant),
                "Infantry {field} must contribute to the state hash"
            );
        }
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
    fn object_falling_byte_changes_state_hash() {
        let mut base = GameEntity::test_default(1, "MTNK", "Americans", 5, 5);
        let base_hash = hash_entity(base.clone());
        base.set_object_is_falling_down_for_test(0xa5);

        assert_ne!(
            base_hash,
            hash_entity(base),
            "ObjectClass falling byte must contribute to the state hash"
        );
    }
}

#[cfg(test)]
mod track_authority_hash_tests {
    use super::Simulation;
    use crate::sim::components::{
        DriveCoord, DriveLocomotionRuntime, ShipLocomotionRuntime, TrackProgress,
    };
    use crate::sim::game_entity::GameEntity;

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
        if ship {
            entity.ship_locomotion = Some(ShipLocomotionRuntime {
                track,
                head_to,
                track_valid: active,
                ..Default::default()
            });
        } else {
            entity.drive_locomotion = Some(DriveLocomotionRuntime {
                track,
                head_to,
                track_valid: active,
                ..Default::default()
            });
        }
        sim.substrate.entities.insert(entity);
        sim
    }

    #[test]
    fn current_hash_covers_each_retained_track_progress_field() {
        for ship in [false, true] {
            for field in 0..6 {
                let mut sim = supplied_track_world(ship, true);
                let before = sim.state_hash();
                let entity = sim.substrate.entities.get_mut(1).unwrap();
                if field == 5 {
                    if ship {
                        let state = entity.ship_locomotion.as_mut().unwrap();
                        state.turn_latched = !state.turn_latched;
                    } else {
                        let state = entity.drive_locomotion.as_mut().unwrap();
                        state.turn_latched = !state.turn_latched;
                    }
                } else if field == 4 {
                    if ship {
                        let state = entity.ship_locomotion.as_mut().unwrap();
                        state.track_valid = !state.track_valid;
                    } else {
                        let state = entity.drive_locomotion.as_mut().unwrap();
                        state.track_valid = !state.track_valid;
                    }
                } else {
                    let track = if ship {
                        &mut entity.ship_locomotion.as_mut().unwrap().track
                    } else {
                        &mut entity.drive_locomotion.as_mut().unwrap().track
                    };
                    match field {
                        0 => track.turn_index = 1,
                        1 => track.cursor = 1,
                        2 => track.reversed = true,
                        3 => track.residual = 1,
                        _ => unreachable!(),
                    }
                }
                assert_ne!(before, sim.state_hash(), "ship={ship} field={field}");
            }
        }
    }

    #[test]
    fn prior_track_free_projection_keeps_retained_residual_and_short_state() {
        for ship in [false, true] {
            let mut sim = supplied_track_world(ship, false);
            let before = sim.state_hash_without_track_authority_v167();
            assert_ne!(
                before,
                sim.state_hash(),
                "obsolete absence tags and pending=false are projected only before schema167"
            );
            let entity = sim.substrate.entities.get_mut(1).unwrap();
            let track = if ship {
                &mut entity.ship_locomotion.as_mut().unwrap().track
            } else {
                &mut entity.drive_locomotion.as_mut().unwrap().track
            };
            // Completion retires selector/cursor independently of these fields.
            track.cursor = 0;
            track.residual = 3;
            track.reversed = true;
            assert_ne!(before, sim.state_hash_without_track_authority_v167());
        }
    }

    #[test]
    #[should_panic(expected = "pre-v167 detached-track projection requires")]
    fn prior_projection_rejects_active_drive_instead_of_fabricating_absence() {
        supplied_track_world(false, true).state_hash_without_track_authority_v167();
    }

    #[test]
    #[should_panic(expected = "pre-v167 detached-track projection requires")]
    fn prior_projection_rejects_active_ship_instead_of_fabricating_absence() {
        supplied_track_world(true, true).state_hash_without_track_authority_v167();
    }
}

#[cfg(test)]
mod rally_hash_tests {
    use super::{Simulation, hash_house_ai_activation_fields};
    use crate::sim::components::{DriveCoord, DriveLocomotionRuntime};
    use crate::sim::game_entity::GameEntity;

    #[test]
    fn entity_rally_target_changes_state_hash() {
        let mut sim_a = Simulation::new();
        let mut sim_b = Simulation::new();
        sim_a
            .substrate
            .entities
            .insert(GameEntity::test_default(1, "GAWEAP", "Americans", 10, 10));
        sim_b
            .substrate
            .entities
            .insert(GameEntity::test_default(1, "GAWEAP", "Americans", 10, 10));

        sim_b.substrate.entities.get_mut(1).unwrap().rally_target = Some((30, 31));

        assert_ne!(sim_a.state_hash(), sim_b.state_hash());
    }

    #[test]
    fn drive_locomotion_state_changes_state_hash() {
        let mut sim_a = Simulation::new();
        let mut sim_b = Simulation::new();
        let entity_a = GameEntity::test_default(1, "AMCV", "Americans", 10, 10);
        let mut entity_b = entity_a.clone();
        let mut drive = DriveLocomotionRuntime::default();
        drive.destination = Some(DriveCoord::cell(45, 40, 0));
        entity_b.navigation.path_replay.directions = vec![2, 2, 2, 2, 2];
        drive.track.residual = 3;
        entity_b.drive_locomotion = Some(drive);
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
        assert_ne!(
            baseline.state_hash(),
            baseline.state_hash_without_base_plan_v110(),
            "the v110 schema folds BasePlan state even when earlier schema state is unchanged"
        );
        assert_eq!(
            baseline.state_hash_without_base_plan_v110(),
            reversed.state_hash_without_base_plan_v110(),
            "the v109 provenance schema excludes BasePlan node order"
        );
        assert_eq!(
            baseline.state_hash_before_lifecycle_v28_and_mission_v29(),
            reversed.state_hash_before_lifecycle_v28_and_mission_v29(),
            "historical schemas exclude the complete v110 authority"
        );

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
    fn base_plan_center_affects_only_current_v111_hash_schema() {
        use crate::sim::house_state::HouseState;

        fn fixture(center: (u16, u16)) -> Simulation {
            let mut sim = Simulation::new();
            let owner = sim.interner.intern("Americans");
            let mut house = HouseState::new(owner, 0, Some(owner), false, 0, 10);
            house.base_plan_center = center;
            sim.houses.insert(owner, house);
            sim
        }

        let baseline = fixture((0, 0));
        let changed = fixture((19, 21));

        assert_ne!(baseline.state_hash(), changed.state_hash());
        assert_eq!(
            baseline.state_hash_without_base_plan_center_v111(),
            changed.state_hash_without_base_plan_center_v111()
        );
    }

    /// `HouseClass+0x242` (the sticky harvester no-ore latch) is folded once,
    /// under the v132 flag only.
    #[test]
    fn house_harvester_no_ore_affects_only_current_v132_hash_schema() {
        use crate::sim::house_state::HouseState;

        fn fixture(latched: bool) -> Simulation {
            let mut sim = Simulation::new();
            let owner = sim.interner.intern("Americans");
            let mut house = HouseState::new(owner, 0, Some(owner), true, 0, 10);
            house.harvester_no_ore = latched;
            sim.houses.insert(owner, house);
            sim
        }

        let baseline = fixture(false);
        let latched = fixture(true);

        assert_ne!(baseline.state_hash(), latched.state_hash());
        assert_eq!(
            baseline.state_hash_without_house_harvester_no_ore_v132(),
            latched.state_hash_without_house_harvester_no_ore_v132()
        );
    }

    /// The v133 House EVA advice folds move only the current schema; the
    /// dedicated pre-v133 probe reproduces the v132 layout.
    #[test]
    fn house_eva_advice_affects_only_current_v133_hash_schema() {
        use crate::sim::house_state::{HouseFrameTimer, HouseState};

        fn fixture(timer: HouseFrameTimer, guard: bool) -> Simulation {
            let mut sim = Simulation::new();
            let owner = sim.interner.intern("Americans");
            let mut house = HouseState::new(owner, 0, Some(owner), true, 0, 10);
            house.eva_funds_timer = timer;
            house.eva_low_power_guard = guard;
            sim.houses.insert(owner, house);
            sim
        }

        let baseline = fixture(HouseFrameTimer::default(), false);
        let armed = fixture(
            HouseFrameTimer {
                start_frame: 40,
                duration: 2_880,
            },
            false,
        );
        let guarded = fixture(HouseFrameTimer::default(), true);

        assert_ne!(baseline.state_hash(), armed.state_hash());
        assert_ne!(baseline.state_hash(), guarded.state_hash());
        assert_eq!(
            baseline.state_hash_without_house_eva_v133(),
            armed.state_hash_without_house_eva_v133()
        );
        assert_eq!(
            baseline.state_hash_without_house_eva_v133(),
            guarded.state_hash_without_house_eva_v133()
        );
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
        assert_ne!(
            baseline.state_hash_without_house_update_activation_v113(),
            production.state_hash_without_house_update_activation_v113()
        );
        assert_eq!(
            baseline.state_hash_without_house_update_activation_v113(),
            autocreate.state_hash_without_house_update_activation_v113()
        );
        assert_ne!(
            baseline.state_hash_without_house_update_activation_v113(),
            ai_triggers.state_hash_without_house_update_activation_v113()
        );
        assert_eq!(
            baseline.state_hash_without_house_deploy_latches_v112(),
            production.state_hash_without_house_deploy_latches_v112()
        );
        assert_eq!(
            baseline.state_hash_without_house_deploy_latches_v112(),
            autocreate.state_hash_without_house_deploy_latches_v112()
        );
        assert_eq!(
            baseline.state_hash_without_house_deploy_latches_v112(),
            ai_triggers.state_hash_without_house_deploy_latches_v112()
        );
        assert_eq!(
            baseline.state_hash_without_house_deploy_latches_v112(),
            auto_base.state_hash_without_house_deploy_latches_v112()
        );
    }

    #[test]
    fn house_ai_activation_hash_field_order_preserves_v113_and_v112_streams() {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        use crate::sim::house_state::{HouseAiActivationLatches, HouseState};

        for (current_iq, production, autocreate_allowed, ai_triggers_active) in [
            (0x1357_2468, true, false, false),
            (-0x0246_1357, false, true, false),
            (0x1020_3040, false, false, true),
        ] {
            let mut house = HouseState::new(Default::default(), 0, None, false, 0, 10);
            house.current_iq = current_iq;
            house.ai_activation = HouseAiActivationLatches {
                production,
                autocreate_allowed,
                ai_triggers_active,
                auto_base_building: true,
            };

            let mut actual_v113 = DefaultHasher::new();
            hash_house_ai_activation_fields(&house, true, true, &mut actual_v113);
            let mut manual_v113 = DefaultHasher::new();
            house.ai_activation.production.hash(&mut manual_v113);
            house
                .ai_activation
                .autocreate_allowed
                .hash(&mut manual_v113);
            house
                .ai_activation
                .ai_triggers_active
                .hash(&mut manual_v113);
            house.current_iq.hash(&mut manual_v113);
            assert_eq!(actual_v113.finish(), manual_v113.finish());

            let mut actual_v112 = DefaultHasher::new();
            hash_house_ai_activation_fields(&house, true, false, &mut actual_v112);
            let mut manual_v112 = DefaultHasher::new();
            house.current_iq.hash(&mut manual_v112);
            house.ai_activation.production.hash(&mut manual_v112);
            house
                .ai_activation
                .ai_triggers_active
                .hash(&mut manual_v112);
            assert_eq!(actual_v112.finish(), manual_v112.finish());

            let mut actual_pre_v112 = DefaultHasher::new();
            hash_house_ai_activation_fields(&house, false, false, &mut actual_pre_v112);
            let mut manual_pre_v112 = DefaultHasher::new();
            house.current_iq.hash(&mut manual_pre_v112);
            assert_eq!(actual_pre_v112.finish(), manual_pre_v112.finish());
        }
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
        entity.base_defense_response.archive_target =
            Some(crate::sim::combat::TargetKind::Entity(7));
        entity.base_defense_response.cooldown_start_frame = 12;
        entity.base_defense_response.cooldown_duration_frames = 225;
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
            directionless: false,
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
            previous_coords: IVec3::ZERO,
            origin: IVec3::ZERO,
            direction: [SimFixed::from_num(0); 3],
            velocity: SimFixed::from_num(0),
            lifetime_remaining: 100,
            damage_counter: 0,
            state_ai_advance: 4,
            animation_state: 0,
            translucency: 0,
            hit_ground: false,
            marked_for_deletion: false,
            drift_x: 0,
            drift_y: 0,
            drift_z: 0,
            current_color: [0; 3],
            color_index: 0,
            color_accumulator: SimFixed::from_num(0),
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
            previous_coords: IVec3::ZERO,
            origin: IVec3::ZERO,
            direction: [SimFixed::from_num(0); 3],
            velocity: SimFixed::from_num(0),
            lifetime_remaining: 9,
            damage_counter: 0,
            state_ai_advance: 0,
            animation_state: 0,
            translucency: 0,
            hit_ground: false,
            marked_for_deletion: false,
            drift_x: 0,
            drift_y: 0,
            drift_z: 0,
            current_color: [0; 3],
            color_index: 0,
            color_accumulator: SimFixed::from_num(0),
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
    fn terrain_spawners_included_in_state_hash() {
        use crate::sim::terrain_spawn::TerrainSpawnerState;

        let mut sim_a = Simulation::new();
        let sim_b = Simulation::new();
        let type_ref = sim_a.interner.intern("TIBTRE01");
        sim_a
            .production
            .terrain_spawners
            .insert((10, 10), TerrainSpawnerState::new(type_ref, 3000, 3, 22));

        assert_ne!(
            sim_a.state_hash(),
            sim_b.state_hash(),
            "terrain_spawners must affect state hash",
        );
    }

    #[test]
    fn terrain_spawner_active_fields_change_state_hash() {
        use crate::sim::terrain_spawn::{TerrainSpawnerPhase, TerrainSpawnerState};

        let mut sim_a = Simulation::new();
        let mut sim_b = Simulation::new();
        let type_ref = sim_a.interner.intern("TIBTRE01");
        let state = TerrainSpawnerState::new(type_ref, 3000, 3, 22);
        sim_a
            .production
            .terrain_spawners
            .insert((10, 10), state.clone());
        sim_b.production.terrain_spawners.insert((10, 10), state);
        assert_eq!(sim_a.state_hash(), sim_b.state_hash());

        let spawner_b = sim_b
            .production
            .terrain_spawners
            .get_mut(&(10, 10))
            .unwrap();
        spawner_b.phase = TerrainSpawnerPhase::Active {
            current_frame: 1,
            ticks_until_next_frame: 2,
        };
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
mod special_locomotor_hash_tests {
    use super::Simulation;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::movement::drop_pod_movement::{DropPodPhase, DropPodState};
    use crate::sim::movement::tunnel_movement::{TunnelPhase, TunnelState};
    use crate::util::fixed_math::SimFixed;

    #[test]
    fn tunnel_and_drop_pod_runtime_change_the_lockstep_hash() {
        fn fixture() -> Simulation {
            let mut sim = Simulation::new();
            sim.substrate
                .entities
                .insert(GameEntity::test_default(1, "MTNK", "Americans", 5, 5));
            sim
        }

        let base = fixture();
        let hash_without_special_runtime = base.state_hash();

        let mut tunnel = fixture();
        tunnel.substrate.entities.get_mut(1).unwrap().tunnel_state = Some(TunnelState {
            phase: TunnelPhase::UndergroundTravel,
        });
        assert_ne!(hash_without_special_runtime, tunnel.state_hash());

        let mut pod = fixture();
        pod.substrate.entities.get_mut(1).unwrap().drop_pod_state = Some(DropPodState {
            phase: DropPodPhase::Descending,
            target_rx: 7,
            target_ry: 9,
            altitude: SimFixed::from_num(100),
            descent_speed: SimFixed::from_num(3),
            elapsed_frames: 4,
        });
        assert_ne!(hash_without_special_runtime, pod.state_hash());
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
    use crate::sim::combat::{AttackTarget, PendingInfantryFire};
    use crate::sim::components::Health;
    use crate::sim::game_entity::{GameEntity, InfantryRuntime};

    fn infantry_entity(sim: &mut Simulation) -> GameEntity {
        GameEntity::new_at_frame_zero_for_test(
            1,
            0,
            0,
            0,
            0,
            sim.interner.intern("Allies"),
            Health { current: 100 },
            sim.interner.intern("E1"),
            EntityCategory::Infantry,
            0,
            5,
            false,
        )
    }

    fn hash_with_animation(animation: Option<Animation>) -> u64 {
        let mut sim = Simulation::new();
        let mut entity = infantry_entity(&mut sim);
        entity.animation = animation;
        sim.substrate.entities.insert(entity);
        sim.state_hash()
    }

    #[test]
    fn every_gameplay_read_animation_field_changes_state_hash() {
        let absent = hash_with_animation(None);
        let base = Animation::new(SequenceKind::Stand);
        let base_hash = hash_with_animation(Some(base.clone()));
        assert_ne!(absent, base_hash);

        let mut sequence = base.clone();
        sequence.sequence = SequenceKind::Attack;
        assert_ne!(base_hash, hash_with_animation(Some(sequence)));

        let mut frame_index = base.clone();
        frame_index.frame_index = 1;
        assert_ne!(base_hash, hash_with_animation(Some(frame_index)));

        let mut elapsed_frames = base.clone();
        elapsed_frames.elapsed_frames = 1;
        assert_ne!(base_hash, hash_with_animation(Some(elapsed_frames)));

        let mut finished = base;
        finished.finished = true;
        assert_ne!(base_hash, hash_with_animation(Some(finished)));
    }

    #[test]
    fn infantry_fear_and_prone_change_hash() {
        let mut sim_a = Simulation::new();
        let mut sim_b = Simulation::new();
        let mut a = infantry_entity(&mut sim_a);
        let b = infantry_entity(&mut sim_b);
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
        let mut a = infantry_entity(&mut sim_a);
        let b = infantry_entity(&mut sim_b);
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
        let mut a = infantry_entity(&mut sim_a);
        let b = infantry_entity(&mut sim_b);
        a.infantry = Some(InfantryRuntime {
            cell_entry_blocked: true,
            ..InfantryRuntime::new()
        });
        sim_a.substrate.entities.insert(a);
        sim_b.substrate.entities.insert(b);
        assert_ne!(sim_a.state_hash(), sim_b.state_hash());
        // A clear byte folds nothing: prior streams keep their pins.
        let mut sim_c = Simulation::new();
        let c = infantry_entity(&mut sim_c);
        sim_c.substrate.entities.insert(c);
        assert_eq!(sim_b.state_hash(), sim_c.state_hash());
    }

    #[test]
    fn foot_path_runtime_hash_and_snapshot_survive_without_movement_adapter() {
        use crate::sim::components::FootPathRuntime;
        use crate::sim::timer::CdTimer;
        let mut sim = Simulation::new();
        let actor = infantry_entity(&mut sim);
        sim.substrate.entities.insert(actor);
        let before = sim.state_hash();
        let previous = sim.state_hash_with_schema(super::HashSchema::Before(160));
        let retained = FootPathRuntime {
            movement_timer: CdTimer::from_raw(-1, -7),
            blocked_timer: CdTimer::from_raw(i32::MAX - 2, 31),
            path_blocked: true,
            retries_left: u32::MAX,
        };
        sim.substrate
            .entities
            .get_mut(1)
            .unwrap()
            .navigation
            .path_runtime = retained;
        assert_ne!(sim.state_hash(), before);
        assert_eq!(
            sim.state_hash_with_schema(super::HashSchema::Before(160)),
            previous
        );
        let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "foot-path", 0);
        let loaded = crate::sim::snapshot::GameSnapshot::load(&bytes)
            .unwrap()
            .sim;
        let actor = loaded.entities().get(1).unwrap();
        assert!(actor.movement_target.is_none());
        assert_eq!(actor.navigation.path_runtime, retained);
    }

    #[test]
    fn pending_infantry_fire_changes_hash() {
        let mut sim_a = Simulation::new();
        let mut sim_b = Simulation::new();
        let mut a = infantry_entity(&mut sim_a);
        let mut b = infantry_entity(&mut sim_b);
        a.attack_target = Some(AttackTarget::new(99));
        b.attack_target = Some(AttackTarget::new(99));
        sim_a.substrate.entities.insert(a);
        sim_b.substrate.entities.insert(b);
        assert_eq!(sim_a.state_hash(), sim_b.state_hash());

        sim_a
            .substrate
            .entities
            .get_mut(1)
            .unwrap()
            .attack_target
            .as_mut()
            .unwrap()
            .pending_infantry_fire = Some(PendingInfantryFire {
            sequence: SequenceKind::Attack,
            fire_frame: 2,
        });
        assert_ne!(
            sim_a.state_hash(),
            sim_b.state_hash(),
            "pending infantry fire state must affect state hash"
        );
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
mod bridge_overlay_hash_tests {
    use super::Simulation;
    use crate::sim::bridge_state::{
        Axis, BridgeCellRole, BridgeEndpointRecord, BridgeRecordKind, BridgeRuntimeCell,
        BridgeRuntimeState, DamageState,
    };

    fn make_bridge_state_with_overlay(byte: u8) -> BridgeRuntimeState {
        let mut state = BridgeRuntimeState::default();
        state.test_seed_cell(
            2,
            2,
            BridgeRuntimeCell {
                deck_present: true,
                destroyable: true,
                deck_level: 0,
                bridge_group_id: Some(1),
                damage_state: DamageState::Healthy { variant: 0 },
                axis: Some(Axis::NS),
                role: BridgeCellRole::Anchor,
                anchor_span_id: None,
                overlay_byte: byte,
                bridgehead_anchor_class: crate::sim::bridge_state::BridgeheadAnchorClass::Variant0,
            },
        );
        state
    }

    #[test]
    fn overlay_byte_difference_changes_state_hash() {
        let mut sim_a = Simulation::new();
        let mut sim_b = Simulation::new();
        sim_a.bridge_state = Some(make_bridge_state_with_overlay(0x18));
        sim_b.bridge_state = Some(make_bridge_state_with_overlay(0xD2));
        assert_ne!(
            sim_a.state_hash(),
            sim_b.state_hash(),
            "overlay_byte must contribute to state hash",
        );
    }

    #[test]
    fn identical_overlay_bytes_hash_equal() {
        let mut sim_a = Simulation::new();
        let mut sim_b = Simulation::new();
        sim_a.bridge_state = Some(make_bridge_state_with_overlay(0x18));
        sim_b.bridge_state = Some(make_bridge_state_with_overlay(0x18));
        assert_eq!(
            sim_a.state_hash(),
            sim_b.state_hash(),
            "identical bridge states must hash equal",
        );
    }

    #[test]
    fn bridgehead_anchor_class_difference_changes_state_hash() {
        use crate::sim::bridge_state::BridgeheadAnchorClass;
        let mut sim_a = Simulation::new();
        let mut sim_b = Simulation::new();

        let mut state_a = make_bridge_state_with_overlay(0x18);
        let state_b = make_bridge_state_with_overlay(0x18);
        if let Some(cell) = state_a.cell_mut(2, 2) {
            cell.bridgehead_anchor_class = BridgeheadAnchorClass::Damaged;
        }
        sim_a.bridge_state = Some(state_a);
        sim_b.bridge_state = Some(state_b);

        assert_ne!(
            sim_a.state_hash(),
            sim_b.state_hash(),
            "bridgehead_anchor_class must contribute to state hash",
        );
    }

    #[test]
    fn bridge_endpoint_record_kind_difference_changes_state_hash() {
        let mut sim_a = Simulation::new();
        let mut sim_b = Simulation::new();

        let mut state_a = make_bridge_state_with_overlay(0x18);
        let mut state_b = make_bridge_state_with_overlay(0x18);
        let mut record = BridgeEndpointRecord {
            endpoint_a: (1, 1),
            endpoint_b: (4, 1),
            group_id: 1,
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
    use std::collections::BTreeMap;

    #[test]
    fn one_advance_is_one_native_frame_for_any_host_duration() {
        let mut sim = Simulation::new();
        let height_map = BTreeMap::new();
        let host_durations = [0, 1, 22, 67, 1_000, u32::MAX];
        for (index, tick_ms) in host_durations.into_iter().enumerate() {
            sim.advance_tick(&[], None, &height_map, None, None, tick_ms);
            assert_eq!(sim.session.binary_frame, index as u32 + 1);
        }
    }

    #[test]
    fn native_frame_wraps_after_u32_max() {
        let mut sim = Simulation::new();
        let height_map = BTreeMap::new();
        sim.session.binary_frame = u32::MAX;

        sim.advance_tick(&[], None, &height_map, None, None, 22);

        assert_eq!(sim.session.binary_frame, 0);
    }

    #[test]
    fn native_frame_changes_state_hash() {
        let mut sim_a = Simulation::new();
        let sim_b = Simulation::new();
        let height_map = BTreeMap::new();
        sim_a.advance_tick(&[], None, &height_map, None, None, 22);
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
            start_frame: 100,
            duration_frames: 30,
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
mod homing_state_hash_tests {
    use super::Simulation;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::movement::homing_movement::{HomingPhase, HomingState, HomingTarget};
    use crate::util::fixed_math::{SIM_ONE, SIM_ZERO, SimFixed};

    fn make_homing(yaw_bam: u16) -> HomingState {
        HomingState {
            phase: HomingPhase::Cruise,
            target: Some(HomingTarget::Object(42)),
            last_known_rx: 25,
            last_known_ry: 5,
            yaw_bam,
            pitch_bam: 0x4000,
            speed: SimFixed::from_num(30),
            pos_x_cells: SimFixed::from_num(5),
            pos_y_cells: SimFixed::from_num(5),
            altitude: SimFixed::from_num(320),
            vz: SIM_ZERO,
            rot_ini: 60,
            missile_rot_var: SIM_ONE,
            floater: false,
            very_high: false,
            arm_ticks_remaining: 0,
            frame_counter: 0,
            stall_counter: 0,
            stall_ema: SIM_ZERO,
            last_distance_to_target: SIM_ZERO,
            pitch: 0.0,
        }
    }

    #[test]
    fn homing_state_presence_changes_hash() {
        let mut a = Simulation::new();
        let mut b = Simulation::new();
        let a_id = a.substrate.entities.insert(GameEntity::test_default(
            1,
            "AAHeatSeeker2",
            "Allied",
            5,
            5,
        ));
        b.substrate
            .entities
            .insert(GameEntity::test_default(1, "AAHeatSeeker2", "Allied", 5, 5));

        // Hashes match while both bullets lack homing_state.
        assert_eq!(a.state_hash(), b.state_hash());

        // Attaching homing_state to `a` only — hashes must diverge.
        a.substrate.entities.get_mut(a_id).unwrap().homing_state = Some(make_homing(0));
        assert_ne!(a.state_hash(), b.state_hash());
    }

    #[test]
    fn homing_state_yaw_changes_hash() {
        let mut a = Simulation::new();
        let mut b = Simulation::new();
        let a_id = a.substrate.entities.insert(GameEntity::test_default(
            1,
            "AAHeatSeeker2",
            "Allied",
            5,
            5,
        ));
        let b_id = b.substrate.entities.insert(GameEntity::test_default(
            1,
            "AAHeatSeeker2",
            "Allied",
            5,
            5,
        ));
        a.substrate.entities.get_mut(a_id).unwrap().homing_state = Some(make_homing(0));
        b.substrate.entities.get_mut(b_id).unwrap().homing_state = Some(make_homing(0x4000));
        assert_ne!(a.state_hash(), b.state_hash());
    }

    #[test]
    fn homing_object_and_cell_targets_hash_differently() {
        let mut a = Simulation::new();
        let mut b = Simulation::new();
        a.substrate
            .entities
            .insert(GameEntity::test_default(1, "AAHeatSeeker2", "Allied", 5, 5));
        b.substrate
            .entities
            .insert(GameEntity::test_default(1, "AAHeatSeeker2", "Allied", 5, 5));
        a.substrate.entities.get_mut(1).unwrap().homing_state = Some(make_homing(0));
        let mut cell_target = make_homing(0);
        cell_target.target = Some(HomingTarget::Cell { rx: 42, ry: 0 });
        b.substrate.entities.get_mut(1).unwrap().homing_state = Some(cell_target);

        assert_ne!(a.state_hash(), b.state_hash());
    }

    #[test]
    fn homing_state_pitch_excluded_from_hash() {
        // The manual Hash impl on HomingState skips the render-only `pitch`
        // field; mutating it must not change the state hash.
        let mut a = Simulation::new();
        let mut b = Simulation::new();
        let a_id = a.substrate.entities.insert(GameEntity::test_default(
            1,
            "AAHeatSeeker2",
            "Allied",
            5,
            5,
        ));
        let b_id = b.substrate.entities.insert(GameEntity::test_default(
            1,
            "AAHeatSeeker2",
            "Allied",
            5,
            5,
        ));
        a.substrate.entities.get_mut(a_id).unwrap().homing_state = Some(make_homing(0));
        let mut h = make_homing(0);
        h.pitch = 1.234;
        b.substrate.entities.get_mut(b_id).unwrap().homing_state = Some(h);
        assert_eq!(
            a.state_hash(),
            b.state_hash(),
            "render-only pitch must not affect state hash"
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
    use crate::sim::movement::locomotion::piggyback::{LocomotorRuntimePayload, StashedLocomotor};
    use crate::sim::movement::locomotor::{LocomotorState, MovementLayer};
    use crate::sim::occupancy::RawCellKey;

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
        let stored = LocomotorState::for_test_kind(kind);
        entity.locomotor = Some(if stashed {
            let mut host = LocomotorState::for_test_kind(LocomotorKind::Teleport);
            host.piggyback = Some(StashedLocomotor::capture(&stored));
            host
        } else {
            stored
        });
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
            LocomotorRuntimePayload::Hover(head) => *head = Some(coord),
            LocomotorRuntimePayload::Jumpjet(state) => match field {
                0 => state.destination = coord,
                1 => state.moving = true,
                2 => state.phase = 2,
                3 => state.flight.target_height = 7,
                4 => state.flight.current_speed_bits = 1.0f64.to_bits(),
                5 => state.params.speed = 99,
                6 => state.landing_latched = true,
                _ => unreachable!(),
            },
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
                    if stashed {
                        let mut stored = loco.piggyback.take().unwrap().into_runtime();
                        mutate(&mut stored.payload, field);
                        loco.piggyback = Some(StashedLocomotor::from_runtime(stored));
                    } else {
                        mutate(&mut loco.runtime_payload, field);
                    }
                    assert_ne!(
                        before.state_hash(),
                        after.state_hash(),
                        "{kind:?} stashed={stashed} field={field}"
                    );
                    for version in [159, 160, 161] {
                        assert_eq!(
                            before.state_hash_with_schema(HashSchema::Before(version)),
                            after.state_hash_with_schema(HashSchema::Before(version)),
                            "only the declared bridge payload is projected out at {version}",
                        );
                    }
                }
            }
        }
        assert!(HashSchema::Before(161).includes(HashFeature::FootPathRuntime));
        assert!(HashSchema::Before(160).includes(HashFeature::CellMembership));
        assert!(!HashSchema::Before(159).includes(HashFeature::CellMembership));
    }

    #[test]
    fn bridge161_projects_dummy_only_and_keeps_real_owner_identity() {
        let mut sim = Simulation::new();
        let owner = sim.intern("Americans");
        let other = sim.intern("Russians");
        let before = sim.state_hash();
        let projected = sim.state_hash_with_schema(HashSchema::Before(161));
        sim.substrate.raw_cell_occupation.write_infantry(
            RawCellKey::Dummy,
            MovementLayer::Ground,
            4,
            owner,
            true,
        );
        assert_ne!(before, sim.state_hash());
        assert_eq!(
            projected,
            sim.state_hash_with_schema(HashSchema::Before(161))
        );
        sim.substrate
            .raw_cell_occupation
            .mark_ground_infantry(5, 5, 4, owner);
        let real_owner = sim.state_hash_with_schema(HashSchema::Before(161));
        assert_ne!(projected, real_owner);
        sim.substrate
            .raw_cell_occupation
            .mark_ground_infantry(5, 5, 4, other);
        assert_ne!(
            real_owner,
            sim.state_hash_with_schema(HashSchema::Before(161)),
            "historical projection does not omit real owner state or invent the former entity ID"
        );
    }
}

#[cfg(test)]
mod aircraft_dock_hash_tests {
    use super::Simulation;
    use crate::sim::aircraft::AircraftMission;
    use crate::sim::docking::aircraft_dock::AircraftAmmo;
    use crate::sim::game_entity::GameEntity;

    #[test]
    fn aircraft_dock_indices_and_reservations_affect_simulation_hash() {
        let mut sim = Simulation::new();
        let mut entity = GameEntity::test_default(1, "ORCA", "Americans", 0, 0);
        let mut ammo = AircraftAmmo::new(3);
        ammo.target_pad = Some(0);
        entity.aircraft_ammo = Some(ammo);
        entity.aircraft_mission = Some(AircraftMission::DockedIdle {
            airfield_id: 2,
            pad_index: 0,
        });
        sim.substrate.entities.insert(entity);
        let original = sim.state_hash();
        sim.substrate
            .entities
            .get_mut(1)
            .unwrap()
            .aircraft_ammo
            .as_mut()
            .unwrap()
            .target_pad = Some(256);
        assert_ne!(
            sim.state_hash(),
            original,
            "saved ammo pad must not alias low byte"
        );
        sim.substrate
            .entities
            .get_mut(1)
            .unwrap()
            .aircraft_ammo
            .as_mut()
            .unwrap()
            .target_pad = Some(0);
        assert_eq!(sim.state_hash(), original);
        sim.substrate.entities.get_mut(1).unwrap().aircraft_mission =
            Some(AircraftMission::DockedIdle {
                airfield_id: 2,
                pad_index: 256,
            });
        assert_ne!(
            sim.state_hash(),
            original,
            "mission pad must not alias low byte"
        );
        let before_reservation = sim.state_hash();
        sim.production.airfield_docks.try_reserve(2, 1, 300);
        assert_ne!(
            sim.state_hash(),
            before_reservation,
            "reservation admission changes future state"
        );
        let occupied = sim.state_hash();
        sim.production.airfield_docks.release(1);
        assert_ne!(
            sim.state_hash(),
            occupied,
            "release changes future admission"
        );
    }
}
