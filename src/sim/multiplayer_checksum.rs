//! Retail multiplayer frame checksum.
//!
//! This is deliberately separate from [`Simulation::state_hash`]. The retail
//! network checksum is a narrow 32-bit diagnostic fold over class arrays,
//! display-layer order, LogicClass order, and one Scenario RNG sample. Computing
//! it also consumes one additional Scenario RNG sample for the native diagnostic
//! snapshot path.

#[cfg(test)]
use crate::map::entities::EntityCategory;
#[cfg(test)]
use crate::sim::game_entity::GameEntity;
use crate::sim::world::Simulation;
#[cfg(test)]
use crate::util::native_x87::{NativeF32Bits, X87Chop53};

pub const DISPLAY_LAYER_COUNT: usize = 5;

#[cfg(test)]
const RTTI_UNIT: i32 = 1;
#[cfg(test)]
const RTTI_AIRCRAFT: i32 = 2;
const RTTI_ANIM: i32 = 4;
#[cfg(test)]
const RTTI_BUILDING: i32 = 6;
#[cfg(test)]
const RTTI_BULLET: i32 = 8;
#[cfg(test)]
const RTTI_INFANTRY: i32 = 0x0f;
#[cfg(test)]
const RTTI_PARTICLE_SYSTEM: i32 = 0x18;
#[cfg(test)]
const RTTI_TERRAIN: i32 = 0x24;
#[cfg(test)]
const RTTI_WAVE: i32 = 0x240;
const SYNC_EXEMPT_ANIM_ID: i32 = -2;
#[cfg(test)]
const LEPTONS_PER_CELL: i32 = crate::util::lepton::LEPTONS_PER_CELL_I32;
#[cfg(test)]
const CELL_CENTER_LEPTON: i32 = crate::util::lepton::CELL_CENTER_LEPTON_I32;
#[cfg(test)]
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
    #[error("LogicClass checksum entry {0} is absent from every object registry")]
    MissingLogicObject(u64),
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
            .wrapping_add(u32::from(facing_checksum_byte(primary_facing)));
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
            .wrapping_add(u32::from(facing_checksum_byte(primary_facing)))
            .wrapping_add(u32::from(facing_checksum_byte(secondary_facing)));
        self.fold_value(term);
    }

    #[inline]
    pub fn fold_building(&mut self, world_x: i32, world_y: i32, primary_facing: u16) {
        let term = coordinate_term(world_x, world_y)
            .wrapping_add(u32::from(facing_checksum_byte(primary_facing)));
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

/// Convert one live 16-bit FacingClass value to the byte mixed by the checksum.
#[inline]
pub fn facing_checksum_byte(facing: u16) -> u8 {
    ((((u32::from(facing) >> 7).wrapping_add(1)) >> 1) & 0xff) as u8
}

#[inline]
fn coordinate_term(world_x: i32, world_y: i32) -> u32 {
    let x = world_x / 10;
    let y = world_y / 10;
    (x as u32).wrapping_add((y as u32).wrapping_mul(0x1_0000))
}

#[cfg(test)]
#[inline]
fn entity_world_xy(entity: &GameEntity) -> (i32, i32) {
    (
        i32::from(entity.position.rx)
            .wrapping_mul(LEPTONS_PER_CELL)
            .wrapping_add(entity.position.sub_x.to_num::<i32>()),
        i32::from(entity.position.ry)
            .wrapping_mul(LEPTONS_PER_CELL)
            .wrapping_add(entity.position.sub_y.to_num::<i32>()),
    )
}

#[cfg(test)]
#[inline]
fn terrain_world_xy(rx: u16, ry: u16) -> (i32, i32) {
    // gamemd-derived: `TerrainClass__Constructor @ 0x0071BB90` converts the
    // map cell passed to Unlimbo to `(cell * 0x100) + 0x80` on both axes.
    (
        i32::from(rx) * LEPTONS_PER_CELL + CELL_CENTER_LEPTON,
        i32::from(ry) * LEPTONS_PER_CELL + CELL_CENTER_LEPTON,
    )
}

#[cfg(test)]
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

#[cfg(test)]
#[inline]
fn wave_world_xy(wave: &crate::sim::wave::Wave) -> Option<(i32, i32)> {
    // gamemd-derived: `WaveClass__Constructor @ 0x0075E950` Reveals at the
    // coordinate produced by `0x00761640/0x00762070`: source for types 0/3,
    // and ftol(source*1.05f + target*(1-1.05f)) for types 1/2.
    Some((
        wave_object_axis(wave.source.x, wave.target.x, wave.wave_type)?,
        wave_object_axis(wave.source.y, wave.target.y, wave.wave_type)?,
    ))
}

#[cfg(test)]
#[inline]
fn primary_facing(entity: &GameEntity, frame: u32) -> u16 {
    entity
        .body_facing
        .as_ref()
        .map_or(u16::from(entity.facing) << 8, |facing| {
            facing.current(frame)
        })
}

#[cfg(test)]
#[inline]
fn secondary_facing(entity: &GameEntity, frame: u32) -> u16 {
    entity
        .barrel_facing
        .as_ref()
        .map_or(0, |facing| facing.current(frame))
}

#[cfg(test)]
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
    /// `display_layers` must be the five native display vectors in their stored
    /// order. Rust does not currently own those vectors in `sim`, so the
    /// presentation owner supplies the exact point-in-time views.
    /// This method must be called only by the admitted multiplayer-frame path:
    /// it consumes exactly two Scenario RNG samples and therefore must never run
    /// for an offline frame or a diagnostic-only `state_hash()` request.
    #[cfg(test)]
    pub fn compute_retail_multiplayer_checksum(
        &mut self,
        display_layers: [&[ChecksumObject]; DISPLAY_LAYER_COUNT],
    ) -> Result<MultiplayerChecksumFrame, MultiplayerChecksumError> {
        let frame = self.session.binary_frame;
        let mut checksum = RetailChecksumAccumulator::new();

        // Native class-array order is constructor/registration order. Stable
        // object IDs are monotonic construction IDs, so filtering the ordered
        // store preserves each class array's order without a second registry.
        for entity in self.substrate.entities.values_sorted() {
            if entity.category == EntityCategory::Infantry {
                let (x, y) = entity_world_xy(entity);
                checksum.fold_infantry(x, y, primary_facing(entity, frame));
            }
        }
        for entity in self.substrate.entities.values_sorted() {
            if entity.category == EntityCategory::Unit {
                let (x, y) = entity_world_xy(entity);
                checksum.fold_unit(
                    x,
                    y,
                    primary_facing(entity, frame),
                    secondary_facing(entity, frame),
                );
            }
        }
        for entity in self.substrate.entities.values_sorted() {
            if entity.category == EntityCategory::Structure {
                let (x, y) = entity_world_xy(entity);
                checksum.fold_building(x, y, primary_facing(entity, frame));
            }
        }

        if self.session.house_order.len() != self.houses.len() {
            return Err(MultiplayerChecksumError::HouseOrderCoverage {
                registered: self.session.house_order.len(),
                stored: self.houses.len(),
            });
        }
        for (index, owner) in self.session.house_order.iter().copied().enumerate() {
            if self.session.house_order[..index].contains(&owner) {
                return Err(MultiplayerChecksumError::DuplicateHouse(owner));
            }
            let house = self
                .houses
                .get(&owner)
                .ok_or(MultiplayerChecksumError::MissingHouse(owner))?;
            checksum.fold_house(u8::from(house.map_is_clear));
        }

        for layer in display_layers {
            for &object in layer {
                checksum.fold_object(object);
            }
        }

        for &stable_id in self.substrate.logic.as_slice() {
            // gamemd-derived: active `Compute_Game_Sync_Checksum @ 0x0064DAB0`
            // reads ObjectClass +0x9c/+0xa0 directly and folds the full virtual
            // WhatAmI value in stored LogicClass order.
            let object = if let Some(entity) = self.substrate.entities.get(stable_id) {
                let (world_x, world_y) = entity_world_xy(entity);
                ChecksumObject::new(world_x, world_y, entity_rtti(entity.category), 0)
            } else if let Some(anim) = self.substrate.anims.get(stable_id) {
                ChecksumObject::new(
                    anim.world_coord.x,
                    anim.world_coord.y,
                    RTTI_ANIM,
                    anim.native_unique_id,
                )
            } else if let Some(system) = self.substrate.particle_systems.get(stable_id) {
                ChecksumObject::new(system.coords.x, system.coords.y, RTTI_PARTICLE_SYSTEM, 0)
            } else if let Some(terrain) = self.production.terrain_objects.get(&stable_id) {
                let (world_x, world_y) = terrain_world_xy(terrain.rx, terrain.ry);
                ChecksumObject::new(world_x, world_y, RTTI_TERRAIN, 0)
            } else if let Some(projectile) = self.projectiles.get(stable_id) {
                ChecksumObject::new(projectile.position.x, projectile.position.y, RTTI_BULLET, 0)
            } else if let Some(wave) = self.waves.get(stable_id) {
                let (world_x, world_y) =
                    wave_world_xy(wave).ok_or(MultiplayerChecksumError::InvalidWaveType {
                        id: stable_id,
                        wave_type: wave.wave_type,
                    })?;
                ChecksumObject::new(world_x, world_y, RTTI_WAVE, 0)
            } else {
                return Err(MultiplayerChecksumError::MissingLogicObject(stable_id));
            };
            checksum.fold_object(object);
        }

        let checksum_rng_sample = self.scenario_rng.next_u32();
        checksum.fold_value(checksum_rng_sample);
        let diagnostic_rng_sample = self.scenario_rng.next_u32();

        Ok(MultiplayerChecksumFrame {
            value: checksum.value(),
            diagnostic_rng_sample,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ChecksumObject, MultiplayerChecksumError, RetailChecksumAccumulator, SYNC_EXEMPT_ANIM_ID,
        facing_checksum_byte,
    };
    use crate::map::entities::EntityCategory;
    use crate::rules::particle_system_type::ParticleSystemTypeId;
    use crate::sim::anim_class::{AnimDrawRuntime, AnimObject, AnimRuntime, AnimWorldCoord};
    use crate::sim::game_entity::GameEntity;
    use crate::sim::intern::InternedId;
    use crate::sim::particles::ParticleSystem;
    use crate::sim::projectile::{
        ProjectileCollisionPolicy, ProjectileCoord, ProjectilePayload, ProjectileSpawn,
        ProjectileTarget, ProjectileTrajectory, ProjectileVelocity, ProjectileVisualState,
        TargetExpiryPolicy,
    };
    use crate::sim::terrain_object::{TerrainObjectLifecycle, TerrainObjectState};
    use crate::sim::timer::CdTimer;
    use crate::sim::wave::Wave;
    use crate::sim::world::Simulation;
    use crate::util::fixed_math::SimFixed;
    use glam::IVec3;

    const FAMILY_COUNT: usize = 6;

    fn test_anim(stable_id: u64, native_unique_id: i32, x: i32, y: i32) -> AnimObject {
        AnimObject {
            stable_id,
            native_unique_id,
            type_id: InternedId::from_index(0),
            world_coord: AnimWorldCoord { x, y, z: 0 },
            draw_flags: 0,
            z_adjust: 0,
            remap_color: None,
            effective_end: 1,
            effective_loop_end: 1,
            runtime: AnimRuntime {
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
            draw_runtime: AnimDrawRuntime::default(),
            use_cell_drawer: false,
            terrain_attached: false,
            in_logic_vector: false,
            owner_entity: None,
            building_slot: None,
            start_sound_active: false,
            stop_sound_id: None,
        }
    }

    fn test_projectile(origin: ProjectileCoord) -> ProjectileSpawn {
        ProjectileSpawn {
            flat: false,
            source_id: 0,
            origin,
            target: ProjectileTarget::Cell { rx: 0, ry: 0 },
            initial_target_position: ProjectileCoord::new(128, 128, 0),
            payload: ProjectilePayload {
                base_damage: 0,
                warhead: InternedId::from_index(0),
                weapon: InternedId::from_index(0),
                owner: InternedId::from_index(0),
            },
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
        let mut sim = Simulation::with_seed(0x16_11_2026);

        let entity_id = sim.allocate_stable_id();
        let mut entity = GameEntity::test_default(entity_id, "ORCA", "Americans", 3, 4);
        entity.category = EntityCategory::Aircraft;
        sim.substrate.entities.insert(entity);

        let anim_id = sim.allocate_stable_id();
        sim.substrate.anims.insert(test_anim(anim_id, 77, 731, -29));

        let particle_id = sim.allocate_stable_id();
        sim.substrate.particle_systems.insert(ParticleSystem {
            stable_id: particle_id,
            in_logic_vector: false,
            type_id: ParticleSystemTypeId(0),
            coords: IVec3::new(-311, 912, 0),
            offset: IVec3::ZERO,
            particles: Vec::new(),
            spawn_timer: SimFixed::from_num(0),
            lifetime: -1,
            spark_spawn_frames: 0,
            facing: 0,
            directionless: true,
            attached_entity: None,
            owner_entity: None,
            target_coords: IVec3::ZERO,
            owner_house: None,
            done_spawning: false,
        });

        let terrain_id = sim.allocate_stable_id();
        sim.production.terrain_objects.insert(
            terrain_id,
            TerrainObjectState {
                stable_id: terrain_id,
                native_unique_id: None,
                in_logic_vector: false,
                type_ref: InternedId::from_index(0),
                rx: 7,
                ry: 9,
                health: 10,
                max_health: 10,
                occupation_bits: 0,
                lifecycle: TerrainObjectLifecycle::Live,
            },
        );

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

        assert_eq!(facing_checksum_byte(0x007f), 0);
        assert_eq!(facing_checksum_byte(0x0080), 1);
        assert_eq!(facing_checksum_byte(0xffff), 0);
    }

    #[test]
    fn only_the_multiplayer_feedback_anim_sentinel_is_skipped() {
        let mut checksum = RetailChecksumAccumulator::new();
        checksum.fold_object(ChecksumObject::new(100, 200, 4, SYNC_EXEMPT_ANIM_ID));
        assert_eq!(checksum.value(), 0);

        checksum.fold_object(ChecksumObject::new(100, 200, 4, -1));
        assert_ne!(checksum.value(), 0);
    }

    #[test]
    fn multiplayer_frame_consumes_exactly_two_scenario_rng_samples() {
        let mut sim = Simulation::with_seed(0x1234);
        let mut reference = sim.scenario_rng.clone();
        let checksum_sample = reference.next_u32();
        let diagnostic_sample = reference.next_u32();

        let frame = sim
            .compute_retail_multiplayer_checksum([&[], &[], &[], &[], &[]])
            .unwrap();

        assert_eq!(frame.value, checksum_sample);
        assert_eq!(frame.diagnostic_rng_sample, diagnostic_sample);
        assert_eq!(sim.scenario_rng.next_u32(), reference.next_u32());
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

        let first_frame = first
            .compute_retail_multiplayer_checksum([&[], &[], &[], &[], &[]])
            .unwrap();
        let second_frame = second
            .compute_retail_multiplayer_checksum([&[], &[], &[], &[], &[]])
            .unwrap();

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

        let frame = sim
            .compute_retail_multiplayer_checksum([&[], &[], &[], &[], &[]])
            .unwrap();

        assert_eq!(frame.value, checksum_sample);
        assert_eq!(frame.diagnostic_rng_sample, diagnostic_sample);
        assert_eq!(sim.scenario_rng.next_u32(), reference.next_u32());
    }

    #[test]
    fn gsi_16_11_dangling_logic_id_remains_a_hard_error() {
        let mut sim = Simulation::new();
        sim.substrate.logic.set_order_for_test(vec![91]);

        assert_eq!(
            sim.compute_retail_multiplayer_checksum([&[], &[], &[], &[], &[]]),
            Err(MultiplayerChecksumError::MissingLogicObject(91))
        );
    }
}
