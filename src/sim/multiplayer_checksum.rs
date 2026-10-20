//! Retail multiplayer frame checksum.
//!
//! This is deliberately separate from [`Simulation::state_hash`]. The retail
//! network checksum is a narrow 32-bit diagnostic fold over class arrays,
//! display-layer order, LogicClass order, and one Scenario RNG sample. Computing
//! it also consumes one additional Scenario RNG sample for the native diagnostic
//! snapshot path.

use crate::map::entities::EntityCategory;
use crate::sim::game_entity::GameEntity;
use crate::sim::world::Simulation;
use crate::util::direction_tables::round_facing16_to_8;
use crate::util::native_x87::{NativeF32Bits, X87Chop53};

const RTTI_UNIT: i32 = 1;
const RTTI_AIRCRAFT: i32 = 2;
const RTTI_ANIM: i32 = 4;
const RTTI_BUILDING: i32 = 6;
const RTTI_BULLET: i32 = 8;
const RTTI_INFANTRY: i32 = 0x0f;
const RTTI_PARTICLE_SYSTEM: i32 = 0x18;
const RTTI_TERRAIN: i32 = 0x24;
/// `VoxelAnimClass` WhatAmI: vtable `0x007F6318` +0x2c is `0x0074AB20`
/// (`MOV EAX, 0x29; RET`).
const RTTI_VOXEL_ANIM: i32 = 0x29;
const RTTI_WAVE: i32 = 0x240;
const SYNC_EXEMPT_ANIM_ID: i32 = -2;
const LEPTONS_PER_CELL: i32 = crate::util::lepton::LEPTONS_PER_CELL_I32;
const CELL_CENTER_LEPTON: i32 = crate::util::lepton::CELL_CENTER_LEPTON_I32;
const WAVE_EXTENDED_ENDPOINT_SCALE: NativeF32Bits = NativeF32Bits::from_bits(0x3f86_6666);

/// One ObjectClass entry as seen by a display-layer checksum pass.
///
/// `native_unique_id` matters only for AnimClass: the multiplayer-only
/// click-feedback sentinel is skipped in both the display and LogicClass folds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChecksumObject {
    pub world_x: i32,
    pub world_y: i32,
    pub what_am_i: i32,
    pub native_unique_id: i32,
}

impl ChecksumObject {
    pub const fn new(world_x: i32, world_y: i32, what_am_i: i32, native_unique_id: i32) -> Self {
        Self {
            world_x,
            world_y,
            what_am_i,
            native_unique_id,
        }
    }

    #[inline]
    const fn is_sync_exempt(self) -> bool {
        self.what_am_i == RTTI_ANIM && self.native_unique_id == SYNC_EXEMPT_ANIM_ID
    }
}

/// The value stored for one multiplayer frame plus the second, diagnostic-only
/// Scenario RNG sample consumed at the same native point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MultiplayerChecksumFrame {
    pub value: u32,
    pub diagnostic_rng_sample: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum MultiplayerChecksumError {
    #[error("house registration order contains duplicate house {0:?}")]
    DuplicateHouse(crate::sim::intern::InternedId),
    #[error("house registration order references missing house {0:?}")]
    MissingHouse(crate::sim::intern::InternedId),
    #[error(
        "house registration order covers {registered} entries but the house store has {stored}"
    )]
    HouseOrderCoverage { registered: usize, stored: usize },
    #[error("checksum display/LogicClass entry {0} is absent from every object registry")]
    MissingObject(u64),
    #[error("LogicClass Wave {id} has invalid native wave type {wave_type}")]
    InvalidWaveType { id: u64, wave_type: u8 },
}

/// Exact 32-bit fold used by the active multiplayer checksum.
///
/// Every input is folded as `rotate_left(acc, 1) + value`, with 32-bit wrapping.
/// Methods are split by native surface so callers cannot accidentally omit the
/// facing terms from the three class-array passes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RetailChecksumAccumulator {
    value: u32,
}

impl RetailChecksumAccumulator {
    pub const fn new() -> Self {
        Self { value: 0 }
    }

    #[inline]
    pub const fn value(self) -> u32 {
        self.value
    }

    #[inline]
    pub fn fold_value(&mut self, value: u32) {
        self.value = self.value.rotate_left(1).wrapping_add(value);
    }

    #[inline]
    pub fn fold_infantry(&mut self, world_x: i32, world_y: i32, primary_facing: u16) {
        let term = coordinate_term(world_x, world_y)
            .wrapping_add(u32::from(round_facing16_to_8(primary_facing)));
        self.fold_value(term);
    }

    #[inline]
    pub fn fold_unit(
        &mut self,
        world_x: i32,
        world_y: i32,
        primary_facing: u16,
        secondary_facing: u16,
    ) {
        let term = coordinate_term(world_x, world_y)
            .wrapping_add(u32::from(round_facing16_to_8(primary_facing)))
            .wrapping_add(u32::from(round_facing16_to_8(secondary_facing)));
        self.fold_value(term);
    }

    #[inline]
    pub fn fold_building(&mut self, world_x: i32, world_y: i32, primary_facing: u16) {
        let term = coordinate_term(world_x, world_y)
            .wrapping_add(u32::from(round_facing16_to_8(primary_facing)));
        self.fold_value(term);
    }

    #[inline]
    pub fn fold_house(&mut self, map_is_clear: u8) {
        self.fold_value(u32::from(map_is_clear));
    }

    #[inline]
    pub fn fold_object(&mut self, object: ChecksumObject) {
        if object.is_sync_exempt() {
            return;
        }
        let term =
            coordinate_term(object.world_x, object.world_y).wrapping_add(object.what_am_i as u32);
        self.fold_value(term);
    }
}

#[inline]
fn coordinate_term(world_x: i32, world_y: i32) -> u32 {
    let x = world_x / 10;
    let y = world_y / 10;
    (x as u32).wrapping_add((y as u32).wrapping_mul(0x1_0000))
}

#[inline]
fn terrain_world_xy(rx: u16, ry: u16) -> (i32, i32) {
    // gamemd-derived: `TerrainClass__Constructor @ 0x0071BB90` converts the
    // map cell passed to Unlimbo to `(cell * 0x100) + 0x80` on both axes.
    (
        i32::from(rx) * LEPTONS_PER_CELL + CELL_CENTER_LEPTON,
        i32::from(ry) * LEPTONS_PER_CELL + CELL_CENTER_LEPTON,
    )
}

#[inline]
fn wave_object_axis(source: i32, target: i32, wave_type: u8) -> Option<i32> {
    let scale = match wave_type {
        0 | 3 => return Some(source),
        1 | 2 => X87Chop53::load_f32(WAVE_EXTENDED_ENDPOINT_SCALE).ok()?,
        _ => return None,
    };
    let complement = X87Chop53::sub(X87Chop53::load_f32(NativeF32Bits::ONE).ok()?, scale);
    let source_term = X87Chop53::mul(X87Chop53::load_i32(source), scale);
    let target_term = X87Chop53::mul(X87Chop53::load_i32(target), complement);
    i32::try_from(X87Chop53::ftol_i64(X87Chop53::add(source_term, target_term)).ok()?).ok()
}

#[inline]
fn wave_world_xy(wave: &crate::sim::wave::Wave) -> Option<(i32, i32)> {
    // gamemd-derived: `WaveClass__Constructor @ 0x0075E950` Reveals at the
    // coordinate produced by `0x00761640/0x00762070`: source for types 0/3,
    // and ftol(source*1.05f + target*(1-1.05f)) for types 1/2.
    Some((
        wave_object_axis(wave.source().x, wave.target().x, wave.wave_type())?,
        wave_object_axis(wave.source().y, wave.target().y, wave.wave_type())?,
    ))
}

#[inline]
fn secondary_facing(entity: &GameEntity, frame: u32) -> u16 {
    entity
        .barrel_facing
        .as_ref()
        .map_or(0, |facing| facing.current(frame))
}

#[inline]
fn entity_rtti(category: EntityCategory) -> i32 {
    match category {
        EntityCategory::Unit => RTTI_UNIT,
        EntityCategory::Infantry => RTTI_INFANTRY,
        EntityCategory::Structure => RTTI_BUILDING,
        EntityCategory::Aircraft => RTTI_AIRCRAFT,
    }
}

impl Simulation {
    /// Compute one active-retail multiplayer checksum.
    ///
    /// gamemd-derived: `Compute_Game_Sync_Checksum @ 0x0064DAB0` folds the
    /// Infantry, Unit and Building class arrays, every House's MapIsClear
    /// byte, the five DisplayClass layer vectors (`0x008A0360`, stride 24) in
    /// stored order, the LogicClass vector, and one Scenario RNG sample, then
    /// `FUN_00650A90` consumes a second sample for its diagnostic snapshot.
    ///
    /// Its only per-frame caller is `Queue_AI_Multiplayer @ 0x006475F0`
    /// (`0x00647684`), which `Queue_AI @ 0x00647260` selects for GameMode
    /// 1..=4 (jump table `0x006475D8`). Campaign and skirmish frames never
    /// call it, so the two RNG draws must never run offline or for a
    /// diagnostic-only `state_hash()` request.
    pub(crate) fn compute_retail_multiplayer_checksum(
        &mut self,
    ) -> Result<MultiplayerChecksumFrame, MultiplayerChecksumError> {
        let objects = self.fold_checksum_objects();
        // Native has no error exit: both Scenario samples are drawn even when
        // VERA cannot resolve an entry, so the stream stays in native step.
        let checksum_rng_sample = self.scenario_rng.next_u32();
        let diagnostic_rng_sample = self.scenario_rng.next_u32();
        let mut checksum = objects?;
        checksum.fold_value(checksum_rng_sample);
        Ok(MultiplayerChecksumFrame {
            value: checksum.value(),
            diagnostic_rng_sample,
        })
    }

    /// Every fold before the Scenario sample, in native order.
    fn fold_checksum_objects(&self) -> Result<RetailChecksumAccumulator, MultiplayerChecksumError> {
        let frame = self.session.binary_frame();
        let mut checksum = RetailChecksumAccumulator::new();

        // Native class-array order is constructor/registration order. Stable
        // object IDs are monotonic construction IDs, so filtering the ordered
        // store preserves each class array's order without a second registry.
        for entity in self.substrate.entities.values_sorted() {
            if entity.category() == EntityCategory::Infantry {
                let [x, y] = crate::sim::movement::ground_pose::position_world_xy(&entity.position);
                checksum.fold_infantry(x, y, entity.body_facing_current(frame));
            }
        }
        for entity in self.substrate.entities.values_sorted() {
            if entity.category() == EntityCategory::Unit {
                let [x, y] = crate::sim::movement::ground_pose::position_world_xy(&entity.position);
                checksum.fold_unit(
                    x,
                    y,
                    entity.body_facing_current(frame),
                    secondary_facing(entity, frame),
                );
            }
        }
        for entity in self.substrate.entities.values_sorted() {
            if entity.category() == EntityCategory::Structure {
                let [x, y] = crate::sim::movement::ground_pose::position_world_xy(&entity.position);
                checksum.fold_building(x, y, entity.body_facing_current(frame));
            }
        }

        if self.houses.order().len() != self.houses.len() {
            return Err(MultiplayerChecksumError::HouseOrderCoverage {
                registered: self.houses.order().len(),
                stored: self.houses.len(),
            });
        }
        for (index, owner) in self.houses.order().iter().copied().enumerate() {
            if self.houses.order()[..index].contains(&owner) {
                return Err(MultiplayerChecksumError::DuplicateHouse(owner));
            }
            let house = self
                .houses
                .get(&owner)
                .ok_or(MultiplayerChecksumError::MissingHouse(owner))?;
            checksum.fold_house(u8::from(house.map_is_clear()));
        }

        // Layers 0..4 in index order, each in its stored member order.
        for &stable_id in self.display_layers().ordered_ids() {
            checksum.fold_object(self.checksum_object(stable_id)?);
        }
        for &stable_id in self.substrate.logic.as_slice() {
            checksum.fold_object(self.checksum_object(stable_id)?);
        }
        Ok(checksum)
    }

    /// The ObjectClass view both the display-layer and LogicClass folds read.
    ///
    /// gamemd-derived: `0x0064DAB0` reads ObjectClass +0x9c/+0xa0 directly
    /// and folds the full virtual WhatAmI value (vt+0x2c).
    fn checksum_object(&self, stable_id: u64) -> Result<ChecksumObject, MultiplayerChecksumError> {
        use crate::sim::world::ObjectKind;
        let missing = MultiplayerChecksumError::MissingObject(stable_id);
        let object = match self.classify_object(stable_id).ok_or(missing)? {
            ObjectKind::Entity => {
                let entity = self.substrate.entities.get(stable_id).ok_or(missing)?;
                let [world_x, world_y] =
                    crate::sim::movement::ground_pose::position_world_xy(&entity.position);
                ChecksumObject::new(world_x, world_y, entity_rtti(entity.category()), 0)
            }
            ObjectKind::Anim => {
                let anim = self.substrate.anims.get(stable_id).ok_or(missing)?;
                ChecksumObject::new(
                    anim.world_coord().x,
                    anim.world_coord().y,
                    RTTI_ANIM,
                    anim.native_unique_id(),
                )
            }
            ObjectKind::Spotlight => {
                let [x, y, _] = self.spotlight_object_coord(stable_id).ok_or(missing)?;
                //64DAB0 folds raw XY and RTTI19 (original4370B0).
                ChecksumObject::new(x, y, 19, 0)
            }
            ObjectKind::VoxelAnim => {
                // `VoxelAnimClass::AI` refreshes ObjectClass +0x9c from the
                // bounce body every visit; `world_coord` is that conversion.
                let piece = self.substrate.voxel_anims.get(stable_id).ok_or(missing)?;
                let coord = piece.world_coord();
                ChecksumObject::new(coord.x, coord.y, RTTI_VOXEL_ANIM, 0)
            }
            ObjectKind::ParticleSystem => {
                let system = self
                    .substrate
                    .particle_systems
                    .get(stable_id)
                    .ok_or(missing)?;
                ChecksumObject::new(
                    system.coords().x,
                    system.coords().y,
                    RTTI_PARTICLE_SYSTEM,
                    0,
                )
            }
            ObjectKind::Terrain => {
                let terrain = self
                    .terrain_objects
                    .objects()
                    .get(&stable_id)
                    .ok_or(missing)?;
                let (world_x, world_y) = terrain_world_xy(terrain.cell().0, terrain.cell().1);
                ChecksumObject::new(world_x, world_y, RTTI_TERRAIN, 0)
            }
            ObjectKind::Projectile => {
                let projectile = self.projectiles.get(stable_id).ok_or(missing)?;
                ChecksumObject::new(
                    projectile.position().x,
                    projectile.position().y,
                    RTTI_BULLET,
                    0,
                )
            }
            ObjectKind::Wave => {
                let wave = self.waves.get(stable_id).ok_or(missing)?;
                let (world_x, world_y) =
                    wave_world_xy(wave).ok_or(MultiplayerChecksumError::InvalidWaveType {
                        id: stable_id,
                        wave_type: wave.wave_type(),
                    })?;
                ChecksumObject::new(world_x, world_y, RTTI_WAVE, 0)
            }
            // SmudgeClass never joins Logic or a display layer (lifecycle
            // asserts it), so a smudge id here is a registry defect.
            ObjectKind::Smudge => return Err(missing),
        };
        Ok(object)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ChecksumObject, MultiplayerChecksumError, RetailChecksumAccumulator, SYNC_EXEMPT_ANIM_ID,
        round_facing16_to_8,
    };
    use crate::map::entities::EntityCategory;
    use crate::rules::particle_system_type::ParticleSystemTypeId;
    use crate::sim::anim_class::{AnimObject, AnimRuntime, AnimWorldCoord};
    use crate::sim::game_entity::GameEntity;
    use crate::sim::intern::InternedId;
    use crate::sim::particles::ParticleSystem;
    use crate::sim::projectile::{
        ProjectileCollisionPolicy, ProjectileCoord, ProjectilePayload, ProjectileSpawn,
        ProjectileTarget, ProjectileTrajectory, ProjectileVelocity, ProjectileVisualState,
        TargetExpiryPolicy,
    };
    use crate::sim::terrain_object::TerrainObjectState;
    use crate::sim::timer::CdTimer;
    use crate::sim::wave::Wave;
    use crate::sim::world::Simulation;
    use glam::IVec3;

    const FAMILY_COUNT: usize = 6;

    fn test_anim(stable_id: u64, native_unique_id: i32, x: i32, y: i32) -> AnimObject {
        AnimObject::new_unattached_for_test(
            stable_id,
            native_unique_id,
            InternedId::from_index(0),
            AnimWorldCoord { x, y, z: 0 },
            AnimRuntime {
                current_frame: 0,
                frame_step: 1,
                delay_remaining: 0,
                rate_reload: 1,
                frame_timer: CdTimer::default(),
                loop_remaining: 1,
                first_ai_guard: false,
                constructor_reverse: false,
                inactive: false,
                paused: false,
            },
        )
    }

    fn test_projectile(origin: ProjectileCoord) -> ProjectileSpawn {
        ProjectileSpawn {
            native_unique_id: 0,
            line_trail: None,
            flat: false,
            source_id: 0,
            origin,
            target: ProjectileTarget::Cell { rx: 0, ry: 0 },
            initial_target_position: ProjectileCoord::new(128, 128, 0),
            payload: ProjectilePayload::new(0, InternedId::from_index(0), None, false),
            speed_leptons_per_frame: 1,
            velocity: ProjectileVelocity::new(0, 0, 0),
            trajectory: ProjectileTrajectory::Straight,
            guidance: None,
            visual: ProjectileVisualState::new(0, 0, 0),
            arm_frames: 0,
            fuse_frames: None,
            ranged_fuse: false,
            tracks_target: false,
            target_expiry: TargetExpiryPolicy::Expire,
            collision: ProjectileCollisionPolicy::NONE,
        }
    }

    fn six_family_sim(
        order: [usize; FAMILY_COUNT],
    ) -> (Simulation, [ChecksumObject; FAMILY_COUNT]) {
        let mut sim = Simulation::with_seed(0x16112026);

        let entity_id = sim.allocate_stable_id();
        let mut entity = GameEntity::test_default(entity_id, "ORCA", "Americans", 3, 4);
        entity.set_category_for_test(EntityCategory::Aircraft);
        sim.substrate.entities.insert(entity);

        let anim_id = sim.allocate_stable_id();
        sim.substrate.anims.insert(test_anim(anim_id, 77, 731, -29));

        let particle_id = sim.allocate_stable_id();
        sim.substrate.particle_systems.insert({
            let mut system = ParticleSystem::test_fixture(
                particle_id,
                ParticleSystemTypeId(0),
                IVec3::new(-311, 912, 0),
            );
            system.set_facing_for_test(0);
            system
        });

        let terrain_id = sim.allocate_stable_id();
        sim.terrain_objects
            .objects_mut_for_test()
            .insert(terrain_id, {
                let mut terrain =
                    TerrainObjectState::for_test(terrain_id, InternedId::from_index(0), 7, 9);
                terrain.set_health_for_test(10);
                terrain.set_max_health_for_test(10);
                terrain
            });

        let bullet_id = sim.allocate_stable_id();
        sim.projectiles.spawn(
            bullet_id,
            test_projectile(ProjectileCoord::new(1234, -567, 0)),
        );

        let wave_id = sim.allocate_stable_id();
        sim.waves.spawn(
            wave_id,
            Wave::new(
                1,
                ProjectileCoord::new(1000, 2000, 0),
                ProjectileCoord::new(0, 0, 0),
            ),
        );

        let ids = [
            entity_id,
            anim_id,
            particle_id,
            terrain_id,
            bullet_id,
            wave_id,
        ];
        for index in order {
            sim.register_live_object(ids[index]);
        }

        let expected_objects = [
            ChecksumObject::new(3 * 256 + 128, 4 * 256 + 128, 2, 0),
            ChecksumObject::new(731, -29, 4, 77),
            ChecksumObject::new(-311, 912, 0x18, 0),
            ChecksumObject::new(7 * 256 + 128, 9 * 256 + 128, 0x24, 0),
            ChecksumObject::new(1234, -567, 8, 0),
            // Native 1.05f endpoint extension truncates each positive axis.
            ChecksumObject::new(1049, 2099, 0x240, 0),
        ];
        (sim, expected_objects)
    }

    fn expected_logic_checksum(
        objects: [ChecksumObject; FAMILY_COUNT],
        order: [usize; FAMILY_COUNT],
        rng_sample: u32,
    ) -> u32 {
        let mut expected = RetailChecksumAccumulator::new();
        for index in order {
            expected.fold_object(objects[index]);
        }
        expected.fold_value(rng_sample);
        expected.value()
    }

    #[test]
    fn representative_fold_matches_the_native_32_bit_trace() {
        let mut checksum = RetailChecksumAccumulator::new();
        checksum.fold_infantry(125, 245, 0x3f80);
        checksum.fold_unit(-125, 305, 0x8000, 0xffff);
        checksum.fold_building(999, -101, 0x0100);
        checksum.fold_house(1);
        checksum.fold_object(ChecksumObject::new(321, 654, 6, 17));

        assert_eq!(checksum.value(), 0x0289_0a18);
    }

    #[test]
    fn rotate_carry_negative_division_and_facing_rounding_are_exact() {
        let mut checksum = RetailChecksumAccumulator::new();
        checksum.fold_value(0x8000_0000);
        checksum.fold_value(0);
        assert_eq!(checksum.value(), 1, "fold is rotate-left, not shift-left");

        let mut negative = RetailChecksumAccumulator::new();
        negative.fold_object(ChecksumObject::new(-19, -19, 1, 9));
        assert_eq!(
            negative.value(),
            // The truncated coordinate term is -65537; WhatAmI(1) joins it.
            0xffff_0000,
            "signed division truncates toward zero before wrapping"
        );

        assert_eq!(round_facing16_to_8(0x007f), 0);
        assert_eq!(round_facing16_to_8(0x0080), 1);
        assert_eq!(round_facing16_to_8(0xffff), 0);
    }

    #[test]
    fn gsi_16_11_six_family_logic_fold_uses_stored_order_direct_coords_and_full_rtti() {
        let first_order = [5, 1, 4, 0, 3, 2];
        let second_order = [2, 3, 0, 4, 1, 5];
        let (mut first, first_objects) = six_family_sim(first_order);
        let (mut second, second_objects) = six_family_sim(second_order);
        let mut reference = first.scenario_rng.clone();
        let checksum_sample = reference.next_u32();
        let diagnostic_sample = reference.next_u32();

        let first_frame = first.compute_retail_multiplayer_checksum().unwrap();
        let second_frame = second.compute_retail_multiplayer_checksum().unwrap();

        assert_eq!(
            first_frame.value,
            expected_logic_checksum(first_objects, first_order, checksum_sample)
        );
        assert_eq!(
            second_frame.value,
            expected_logic_checksum(second_objects, second_order, checksum_sample)
        );
        assert_ne!(first_frame.value, second_frame.value);
        assert_eq!(first_frame.diagnostic_rng_sample, diagnostic_sample);
        assert_eq!(second_frame.diagnostic_rng_sample, diagnostic_sample);
        assert_eq!(first.scenario_rng.next_u32(), reference.next_u32());
    }

    #[test]
    fn gsi_16_11_only_anim_minus_two_is_exempt_in_the_live_logic_resolver() {
        let mut sim = Simulation::with_seed(0x16_11);
        let anim_id = sim.allocate_stable_id();
        sim.substrate
            .anims
            .insert(test_anim(anim_id, SYNC_EXEMPT_ANIM_ID, 500, 700));
        sim.register_live_object(anim_id);
        let mut reference = sim.scenario_rng.clone();
        let checksum_sample = reference.next_u32();
        let diagnostic_sample = reference.next_u32();

        let frame = sim.compute_retail_multiplayer_checksum().unwrap();

        assert_eq!(frame.value, checksum_sample);
        assert_eq!(frame.diagnostic_rng_sample, diagnostic_sample);
        assert_eq!(sim.scenario_rng.next_u32(), reference.next_u32());
    }

    #[test]
    fn display_layers_fold_in_layer_order_before_the_logic_vector() {
        use crate::sim::world::display_layers::DisplayLayer;
        let mut sim = Simulation::with_seed(0x64da);
        let top = sim.allocate_stable_id();
        sim.substrate.anims.insert(test_anim(top, 5, 40, 50));
        let surface = sim.allocate_stable_id();
        sim.substrate.anims.insert(test_anim(surface, 6, 70, 80));
        let logic = sim.allocate_stable_id();
        sim.substrate.anims.insert(test_anim(logic, 7, 100, 110));
        // Submission order is deliberately the reverse of layer order.
        assert!(
            sim.substrate
                .display
                .submit(top, Some(DisplayLayer::TOP), &|_| 0)
        );
        assert!(
            sim.substrate
                .display
                .submit(surface, Some(DisplayLayer::SURFACE), &|_| 0)
        );
        sim.register_live_object(logic);
        let mut reference = sim.scenario_rng.clone();
        let checksum_sample = reference.next_u32();

        let frame = sim.compute_retail_multiplayer_checksum().unwrap();

        let mut expected = RetailChecksumAccumulator::new();
        expected.fold_object(ChecksumObject::new(70, 80, 4, 6));
        expected.fold_object(ChecksumObject::new(40, 50, 4, 5));
        expected.fold_object(ChecksumObject::new(100, 110, 4, 7));
        expected.fold_value(checksum_sample);
        assert_eq!(frame.value, expected.value());
    }

    #[test]
    fn gsi_16_11_dangling_display_id_is_a_hard_error() {
        let mut sim = Simulation::new();
        assert!(sim.substrate.display.submit(
            92,
            Some(crate::sim::world::display_layers::DisplayLayer::AIR),
            &|_| 0
        ));

        assert_eq!(
            sim.compute_retail_multiplayer_checksum(),
            Err(MultiplayerChecksumError::MissingObject(92))
        );
    }

    #[test]
    fn only_the_multiplayer_queue_route_checksums_a_frame() {
        use crate::sim::world::{CommandQueueRoute, FrameEffects, TickLane};
        let mut single = Simulation::with_seed(0x6476);
        let mut multi = Simulation::with_seed(0x6476);
        multi.set_command_queue_route(CommandQueueRoute::Multiplayer);
        let frame = multi.session.binary_frame();
        let mut expected_rng = single.scenario_rng.clone();

        let advance = |sim: &mut Simulation| {
            sim.advance_app_frame(
                &[],
                None,
                None,
                67,
                TickLane::Ordinary,
                FrameEffects::default(),
            )
            .expect("empty frame completes")
        };
        let single_output = advance(&mut single);
        let multi_output = advance(&mut multi);

        assert_eq!(single_output.multiplayer_checksum, None);
        let sync = multi_output
            .multiplayer_checksum
            .expect("Queue_AI_Multiplayer stores one checksum per frame");
        assert_eq!(sync.frame, frame, "stamped with the pre-commit frame");
        let checksum_sample = expected_rng.next_u32();
        let diagnostic_sample = expected_rng.next_u32();
        assert_eq!(
            sync.checksum,
            Ok(super::MultiplayerChecksumFrame {
                value: checksum_sample,
                diagnostic_rng_sample: diagnostic_sample,
            })
        );
        // The empty frame draws nothing else, so the single-player stream is
        // exactly the multiplayer stream minus the two checksum draws.
        assert_eq!(
            single.rng_state().scenario,
            Simulation::with_seed(0x6476).rng_state().scenario
        );
        assert_eq!(multi.rng_state().scenario, expected_rng.logical_state());
    }

    #[test]
    fn voxel_anim_debris_folds_in_the_air_layer_and_logic_with_whatami_0x29() {
        use crate::rules::ini_parser::IniFile;
        use crate::rules::voxel_anim_type::{VoxelAnimType, VoxelAnimTypeId};
        let mut sim = Simulation::with_seed(0x74ab);
        let id = sim.allocate_stable_id();
        let tire = VoxelAnimType::from_ini_section(
            "TIRE",
            IniFile::from_str("[TIRE]\nDuration=150\n")
                .section("TIRE")
                .unwrap(),
        );
        let piece = crate::sim::voxel_anim::spawn_debris_piece(
            id,
            0,
            VoxelAnimTypeId(0),
            &tire,
            None,
            IVec3::new(1280, 2560, 96),
            &mut crate::sim::rng::SimRng::new(1),
        )
        .unwrap();
        let coord = piece.world_coord();
        sim.substrate.voxel_anims.insert(piece);
        assert!(sim.reveal_voxel_anim(id));
        let mut reference = sim.scenario_rng.clone();

        let frame = sim.compute_retail_multiplayer_checksum().unwrap();

        let object = ChecksumObject::new(coord.x, coord.y, 0x29, 0);
        let mut expected = RetailChecksumAccumulator::new();
        expected.fold_object(object);
        expected.fold_object(object);
        expected.fold_value(reference.next_u32());
        assert_eq!(frame.value, expected.value());
    }

    #[test]
    fn an_unresolved_entry_still_draws_both_scenario_samples() {
        let mut sim = Simulation::with_seed(0x64da);
        sim.substrate.logic.set_order_for_test(vec![93]);
        let mut reference = sim.scenario_rng.clone();
        reference.next_u32();
        reference.next_u32();

        assert!(sim.compute_retail_multiplayer_checksum().is_err());
        assert_eq!(sim.scenario_rng.next_u32(), reference.next_u32());
    }
}
