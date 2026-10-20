//! Particle systems — authoritative sim state for visual + damage particle effects.
//!
//! Two-tier model:
//!   - `ParticleSystem` — container that owns a `Vec<Particle>`, manages spawning,
//!     dispatches per-tick AI based on its `ParticleSystemBehavesLike` type.
//!   - `Particle` — individual entity with position, velocity, lifetime, animation
//!     state, optionally dealing damage to cell occupants (gas / fire variants).
//!
//! Systems live in the shared object substrate and enter its `LogicVector`.
//! Particles never enter global storage or the active-object vector: they are
//! owned by their parent system.
//!
//! Smoke / Gas / Fire use the existing SHP render pipeline. Spark enters the
//! shared live-object dispatch; Railgun admission remains unavailable.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on rules/ and util/ only.
//! - sim/ NEVER depends on render/, ui/, audio/, net/.

use crate::rules::particle_system_type::ParticleSystemTypeId;
use crate::rules::particle_type::{ParticleBehavesLike, ParticleType, ParticleTypeId};
use crate::sim::intern::InternedId;
use crate::sim::touch_log::{TouchLogs, Touched};
use crate::sim::world::Simulation;
use crate::util::fixed_math::{SIM_ZERO, SimFixed};
use crate::util::native_x87::{NativeF32Bits, NativeF64Bits};
use glam::IVec3;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub mod fire;
pub mod gas;
pub mod smoke;
pub mod spark;
pub mod spark_spawn;
pub mod spark_world;
pub mod spawn;
pub mod system_ai;
pub mod wind;

#[cfg(test)]
mod gas_native_tests;
#[cfg(test)]
mod native_fixture;
#[cfg(test)]
mod native_identity_tests;
#[cfg(test)]
mod virus_tests;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParticleSystem {
    stable_id: u64,
    /// AbstractClass+10, independent of the Rust handle. Original630100 calls
    /// Object5F6250 -> Abstract410410 to checksum it;630090 ->410320 saves it.
    native_unique_id: i32,
    /// LogicClass active-vector membership. The serialized vector is
    /// authoritative across load, so this object-local guard is rebuilt.
    #[serde(skip)]
    in_logic_vector: bool,
    type_id: ParticleSystemTypeId,
    #[serde(with = "ivec3_serde")]
    coords: IVec3,
    /// Constructor62DE6D: Location minus owner GetCoords, in world leptons.
    /// Smoke AI adds this unchanged displacement to moving owners' GetCoords.
    #[serde(with = "ivec3_serde")]
    offset: IVec3,
    particles: Vec<Particle>,
    // Native +E8, owned by ParticleSystem: repeatedly stored binary32.
    spawn_timer: NativeF32Bits,
    lifetime: i32,
    spark_spawn_frames: i32,
    facing: u8,
    /// Constructor62DE35..62DE5A: the owner's TarCom when it is a Techno,
    /// otherwise the supplied target only when no owner was supplied.
    target_ref: Option<crate::sim::combat::TargetKind>,
    owner_entity: Option<u64>,
    #[serde(with = "ivec3_serde")]
    target_coords: IVec3,
    owner_house: Option<InternedId>,
    /// `ParticleSystemClass+0xF8` — one byte with two jobs, and this engine
    /// used to model it as two fields that drifted apart.
    ///
    /// gamemd-derived: `ParticleSystemClass::AI @ 0x0062FD60` decrements the
    /// lifetime at `+0xEC` and calls vtable `+0xF8` when it reaches exactly
    /// zero; that entry (`0x006301E0`) does nothing but `*(byte*)(this+0xF8) =
    /// 1`. `AI_Smoke @ 0x0062ED40` sets the same byte at `0x0062F218` when its
    /// spawn accumulator passes `SpawnCutoff`, and reads it at `0x0062F047` to
    /// skip spawning; `AI_Spark @ 0x0062E840` sets it at `0x0062EC73` when the
    /// spark countdown runs out. Removal is then
    /// `alive && this[+0xF8] && particle_count == 0`.
    ///
    /// So "finished spawning" and "marked for removal" are not two states in
    /// native — they are one. Splitting them here let a system with
    /// `Lifetime=-1` (which every stock Spark system has, since none authors
    /// `Lifetime=`) finish spawning, empty out, and then never retire.
    done_spawning: bool,
    alive: bool,
    in_limbo: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SparkRuntimeState {
    pub velocity_x: NativeF32Bits,
    pub velocity_y: NativeF32Bits,
    pub velocity_z: NativeF32Bits,
    pub start_rgb: [u8; 3],
    pub color_index: i32,
    pub color_accumulator: NativeF64Bits,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Particle {
    /// Shared Object handle for its native Display AIR entry.
    stable_id: u64,
    /// Original630100 reads each owned Particle's GetUniqueID for its CRC;
    /// ParticleSave62D810 -> AbstractSave410320 also retains this signed word.
    native_unique_id: i32,
    type_id: ParticleTypeId,
    #[serde(with = "ivec3_serde")]
    coords: IVec3,
    #[serde(with = "ivec3_serde")]
    origin: IVec3,
    /// Constructor's normalized target vector (+10C), retained at the existing
    /// Fire fixed-point boundary. It is distinct from the Gas motion words.
    direction: [SimFixed; 3],
    /// Particle+CC/D0/D4. Constructor62B634 initializes (0,0,-1); Gas AI
    /// replaces these words before Move62D2A0 consumes them.
    motion: [NativeF32Bits; 3],
    // Native +E4. All class bodies mutate this one stored scalar.
    velocity: NativeF32Bits,
    lifetime_remaining: i16,
    damage_counter: i16,
    state_ai_advance: u8,
    animation_state: u8,
    translucency: u8,
    marked_for_deletion: bool,

    drift_x: i32,
    drift_y: i32,
    drift_z: i32,

    /// Authoritative behavior-3 state. Generic direction/velocity fields
    /// remain authoritative for the existing Smoke/Gas/Fire implementations only.
    spark: Option<SparkRuntimeState>,

    /// Fire-only scratch: per-tick velocity delta computed by fire AI and
    /// consumed by `move_fire` (jitter * direction). Zero for smoke/gas.
    prev_delta: [SimFixed; 3],
}

impl Particle {
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_damage_counter_for_test(&mut self, value: i16) {
        self.damage_counter = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_state_ai_advance_for_test(&mut self, value: u8) {
        self.state_ai_advance = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_drift_x_for_test(&mut self, value: i32) {
        self.drift_x = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_drift_y_for_test(&mut self, value: i32) {
        self.drift_y = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_drift_z_for_test(&mut self, value: i32) {
        self.drift_z = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn direction_mut_for_test(&mut self) -> &mut [SimFixed; 3] {
        &mut self.direction
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn motion_mut_for_test(&mut self) -> &mut [NativeF32Bits; 3] {
        &mut self.motion
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn prev_delta_mut_for_test(&mut self) -> &mut [SimFixed; 3] {
        &mut self.prev_delta
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_lifetime_remaining_for_test(&mut self, value: i16) {
        self.lifetime_remaining = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_origin_for_test(&mut self, value: IVec3) {
        self.origin = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_spark_for_test(&mut self, value: Option<SparkRuntimeState>) {
        self.spark = value;
    }

    pub const fn type_id(&self) -> ParticleTypeId {
        self.type_id
    }

    pub const fn coords(&self) -> IVec3 {
        self.coords
    }

    pub const fn origin(&self) -> IVec3 {
        self.origin
    }

    pub const fn direction(&self) -> [SimFixed; 3] {
        self.direction
    }

    pub const fn motion(&self) -> [NativeF32Bits; 3] {
        self.motion
    }

    pub const fn lifetime_remaining(&self) -> i16 {
        self.lifetime_remaining
    }

    pub const fn damage_counter(&self) -> i16 {
        self.damage_counter
    }

    pub const fn state_ai_advance(&self) -> u8 {
        self.state_ai_advance
    }

    pub const fn animation_state(&self) -> u8 {
        self.animation_state
    }

    pub const fn translucency(&self) -> u8 {
        self.translucency
    }

    pub const fn marked_for_deletion(&self) -> bool {
        self.marked_for_deletion
    }

    pub const fn drift_x(&self) -> i32 {
        self.drift_x
    }

    pub const fn drift_y(&self) -> i32 {
        self.drift_y
    }

    pub const fn drift_z(&self) -> i32 {
        self.drift_z
    }

    pub const fn spark(&self) -> Option<SparkRuntimeState> {
        self.spark
    }

    pub const fn prev_delta(&self) -> [SimFixed; 3] {
        self.prev_delta
    }

    pub fn stable_id(&self) -> u64 {
        self.stable_id
    }
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_velocity_for_test(&mut self, velocity: NativeF32Bits) {
        self.velocity = velocity;
    }

    pub fn velocity(&self) -> NativeF32Bits {
        self.velocity
    }

    pub fn native_unique_id(&self) -> i32 {
        self.native_unique_id
    }

    /// Constructor state shared by every particle spawn: the type's velocity,
    /// damage counter, state-AI advance, start state and translucency, with
    /// normalized direction and drift. Original Particle62B5E0 reaches
    /// Abstract410230 -> Scenario68BCB0 before its lifetime draw. Spark, Fire,
    /// Smoke and Gas all use this boundary; children never join global Logic.
    /// Executed constructor/CRC controls: tools/procedural_drawing_oracle/electric_bolt.
    pub(super) fn new(
        type_id: ParticleTypeId,
        coords: IVec3,
        target: IVec3,
        pt: &ParticleType,
        sim: &mut Simulation,
    ) -> Self {
        let particle = Self::new_with_ground(type_id, coords, target, pt, sim, |sim, x, y| {
            spark_world::constructor_ground_height(sim, x, y)
        })
        .expect("particle construction requires supported ground facts");
        Simulation::reveal_particle(
            &sim.interner,
            &sim.terrain_objects,
            &mut sim.substrate,
            particle.stable_id,
        );
        particle
    }

    /// Constructor62B5E0 terrain seam used by the existing Spark controls.
    /// Spark registers only after its conditional color initialization.
    pub(super) fn new_with_ground<E, F>(
        type_id: ParticleTypeId,
        coords: IVec3,
        target: IVec3,
        pt: &ParticleType,
        sim: &mut Simulation,
        mut ground: F,
    ) -> Result<Self, E>
    where
        F: FnMut(&Simulation, i32, i32) -> Result<Option<i32>, E>,
    {
        let native_unique_id = sim.next_native_runtime_id();
        let stable_id = sim.substrate.allocate_stable_id();
        let divisor = if pt.behaves_like == ParticleBehavesLike::Railgun {
            10
        } else {
            pt.max_ec as u32
        };
        // Particle62B7DE..62B811: Smoke alone takes this signed-remainder
        // raw draw before the shared lifetime draw (issue1413).
        let velocity = if pt.behaves_like == ParticleBehavesLike::Smoke {
            let delta = sim.particle_rng().next_raw_modulo_signed(2);
            crate::util::native_x87::MaskedX87Chop53::store_f32_masked_chop(
                crate::util::native_x87::MaskedX87Chop53::add(
                    crate::util::native_x87::MaskedX87Chop53::load_f32(pt.velocity),
                    crate::util::native_x87::MaskedX87Chop53::load_i32(delta),
                ),
            )
        } else {
            pt.velocity
        };
        assert_ne!(divisor, 0, "native Particle constructor divides by MaxEC");
        let lifetime_extra = sim.particle_rng().next_raw_abs_modulo(divisor) as i16;
        let lifetime_remaining = (pt.max_ec as i16).wrapping_add(lifetime_extra);
        let location = floor_constructor_coords_with(coords, |x, y| ground(sim, x, y))?;
        let direction = spawn::native_velocity(coords, target);
        let advance = spawn::spawn_state_ai_advance(pt, coords, target, direction, velocity);
        Ok(Particle {
            stable_id,
            native_unique_id,
            type_id,
            coords: location,
            origin: coords,
            direction: direction.map(|component| {
                SimFixed::from_bits((f32::from_bits(component.bits()) * 65536.0) as i32)
            }),
            motion: [
                NativeF32Bits::POSITIVE_ZERO,
                NativeF32Bits::POSITIVE_ZERO,
                NativeF32Bits::from_bits(0xbf80_0000),
            ],
            velocity,
            lifetime_remaining,
            damage_counter: pt.max_dc as i16,
            state_ai_advance: advance,
            animation_state: pt.start_state_ai,
            translucency: pt.translucency,
            marked_for_deletion: false,
            drift_x: 0,
            drift_y: 0,
            drift_z: 0,
            spark: None,
            prev_delta: [SIM_ZERO; 3],
        })
    }
}

impl ParticleSystem {
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_coords_for_test(&mut self, value: IVec3) {
        self.coords = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_done_spawning_for_test(&mut self, value: bool) {
        self.done_spawning = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_facing_for_test(&mut self, value: u8) {
        self.facing = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_in_logic_vector_for_test(&mut self, value: bool) {
        self.in_logic_vector = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_lifetime_for_test(&mut self, value: i32) {
        self.lifetime = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_offset_for_test(&mut self, value: IVec3) {
        self.offset = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_owner_entity_for_test(&mut self, value: Option<u64>) {
        self.owner_entity = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_owner_house_for_test(&mut self, value: Option<InternedId>) {
        self.owner_house = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_particles_for_test(&mut self, value: Vec<Particle>) {
        self.particles = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_spark_spawn_frames_for_test(&mut self, value: i32) {
        self.spark_spawn_frames = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_stable_id_for_test(&mut self, value: u64) {
        self.stable_id = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_target_coords_for_test(&mut self, value: IVec3) {
        self.target_coords = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_target_ref_for_test(&mut self, value: Option<crate::sim::combat::TargetKind>) {
        self.target_ref = value;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn particles_mut_for_test(&mut self) -> &mut Vec<Particle> {
        &mut self.particles
    }

    /// LogicClass membership: RegisterObject sets it (`0x0055BAC6`),
    /// UnregisterObject clears it (`0x0055BB27`). Only the logic vector's
    /// owner writes it, and a load rebuilds it from the saved vector.
    pub(crate) fn mark_logic_membership(&mut self, member: bool) {
        self.in_logic_vector = member;
    }

    /// `0x006301F0`, which the refinery smoke calls right after each
    /// constructor (`0x004599CD`): the system lives `lifetime` frames.
    pub(crate) fn override_lifetime(&mut self, lifetime: i32) {
        self.lifetime = lifetime;
    }

    pub const fn stable_id(&self) -> u64 {
        self.stable_id
    }

    pub const fn in_logic_vector(&self) -> bool {
        self.in_logic_vector
    }

    pub const fn type_id(&self) -> ParticleSystemTypeId {
        self.type_id
    }

    pub const fn coords(&self) -> IVec3 {
        self.coords
    }

    pub const fn offset(&self) -> IVec3 {
        self.offset
    }

    pub fn particles(&self) -> &[Particle] {
        &self.particles
    }

    pub const fn lifetime(&self) -> i32 {
        self.lifetime
    }

    pub const fn spark_spawn_frames(&self) -> i32 {
        self.spark_spawn_frames
    }

    pub const fn facing(&self) -> u8 {
        self.facing
    }

    pub const fn target_ref(&self) -> Option<crate::sim::combat::TargetKind> {
        self.target_ref
    }

    pub const fn owner_entity(&self) -> Option<u64> {
        self.owner_entity
    }

    pub const fn target_coords(&self) -> IVec3 {
        self.target_coords
    }

    pub const fn owner_house(&self) -> Option<InternedId> {
        self.owner_house
    }

    pub const fn done_spawning(&self) -> bool {
        self.done_spawning
    }

    /// ParticleSystem virtual F8 ->6301E0: set the native completion byte.
    /// Subtype AI controls emission; SystemAI62FD60 owns done-and-empty
    /// retirement. Existing children, Alive and expiry broadcasts stay intact.
    pub(crate) fn mark_done_spawning(&mut self) {
        self.done_spawning = true;
    }

    /// The ids [`ParticleSystemStore::pointer_expired`] compares: the target
    /// (`+0xE4`), owner (`+0xE0`) and child vector (`+0xBC`). Feeds the pointer-expiry
    /// listener index (`ExpiryListeners`); a field that callback compares
    /// must be named here too.
    pub(crate) fn expiry_names(&self) -> impl Iterator<Item = Option<u64>> + '_ {
        [
            match self.target_ref {
                Some(crate::sim::combat::TargetKind::Entity(id)) => Some(id),
                _ => None,
            },
            self.owner_entity,
        ]
        .into_iter()
        .chain(
            self.particles
                .iter()
                .map(|particle| Some(particle.stable_id())),
        )
    }

    /// SystemPointerExpired62FEA4..62FEE2 stable-removes the matching
    /// child reference. Rust transfers the uniquely owned body to the
    /// retained store rather than freeing it with that vector element.
    /// Source/control: procedural_drawing_oracle/welding live cleanup.
    pub(crate) fn remove_expired_child(&mut self, expired: u64) -> Option<Particle> {
        let index = self
            .particles
            .iter()
            .position(|particle| particle.stable_id() == expired)?;
        Some(self.particles.remove(index))
    }

    pub fn alive(&self) -> bool {
        self.alive
    }
    pub fn in_limbo(&self) -> bool {
        self.in_limbo
    }
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_spawn_timer_for_test(&mut self, value: NativeF32Bits) {
        self.spawn_timer = value;
    }

    pub fn spawn_timer(&self) -> NativeF32Bits {
        self.spawn_timer
    }

    pub fn native_unique_id(&self) -> i32 {
        self.native_unique_id
    }

    /// Declared synthetic state for tests that exercise storage or AI directly.
    /// Runtime construction always goes through `Simulation::spawn_particle_system`.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn test_fixture(
        stable_id: u64,
        type_id: ParticleSystemTypeId,
        coords: IVec3,
    ) -> Self {
        Self {
            stable_id,
            native_unique_id: 0,
            in_logic_vector: false,
            type_id,
            coords,
            offset: IVec3::ZERO,
            particles: Vec::new(),
            spawn_timer: NativeF32Bits::ONE,
            lifetime: -1,
            spark_spawn_frames: 0,
            facing: 0x1D,
            target_ref: None,
            owner_entity: None,
            target_coords: IVec3::ZERO,
            owner_house: None,
            done_spawning: false,
            alive: true,
            in_limbo: false,
        }
    }
}

#[cfg(any(test, feature = "test-support"))]
impl Particle {
    /// Synthetic particle state without allocating a runtime identity or RNG draw.
    pub(crate) fn test_fixture(type_id: ParticleTypeId, coords: IVec3) -> Self {
        Self {
            stable_id: 0,
            native_unique_id: 0,
            type_id,
            coords,
            origin: coords,
            direction: [SIM_ZERO; 3],
            motion: [NativeF32Bits::POSITIVE_ZERO; 3],
            velocity: NativeF32Bits::POSITIVE_ZERO,
            lifetime_remaining: 0,
            damage_counter: 0,
            state_ai_advance: 0,
            animation_state: 0,
            translucency: 0,
            marked_for_deletion: false,
            drift_x: 0,
            drift_y: 0,
            drift_z: 0,
            spark: None,
            prev_delta: [SIM_ZERO; 3],
        }
    }
}

// ParticleCtor62B8B1/62B8C2: repeat the lookup only on the floor arm.
// Missing-terrain None is the existing fixture policy; retail always has a map.
pub(super) fn floor_constructor_coords_with<E, F>(
    system_coords: IVec3,
    mut ground_height: F,
) -> Result<IVec3, E>
where
    F: FnMut(i32, i32) -> Result<Option<i32>, E>,
{
    let first_ground = ground_height(system_coords.x, system_coords.y)?;
    let z = if first_ground.is_some_and(|ground| system_coords.z <= ground) {
        ground_height(system_coords.x, system_coords.y)?.unwrap_or(system_coords.z)
    } else {
        system_coords.z
    };
    Ok(IVec3::new(system_coords.x, system_coords.y, z))
}

mod ivec3_serde {
    use glam::IVec3;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub(super) fn serialize<S>(value: &IVec3, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        [value.x, value.y, value.z].serialize(serializer)
    }

    pub(super) fn deserialize<'de, D>(deserializer: D) -> Result<IVec3, D::Error>
    where
        D: Deserializer<'de>,
    {
        let [x, y, z] = <[i32; 3]>::deserialize(deserializer)?;
        Ok(IVec3::new(x, y, z))
    }
}

/// Deterministic store for `ParticleSystem` instances.
///
/// BTreeMap-backed so storage iteration is deterministic.
/// Identity is assigned by `ObjectSubstrate`; this store deliberately has no
/// allocator of its own. Every `&mut ParticleSystem` goes through the store,
/// which notes it in a touch log.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ParticleSystemStore {
    systems: BTreeMap<u64, ParticleSystem>,
    /// Scenario setup's DAT_A8ED78. The system/children remain in the one
    /// ordinary store; this is its saved singleton reference, not another body.
    gas_cloud_system: Option<u64>,
    /// Exclusive ownership after SystemPointerExpired removes a child
    /// reference. Reference removal alone preserves Object+90 and Display;
    /// ordinary UnInit then conceals it, clears Alive and queues destruction.
    /// The shared DeferredDeleteQueue owns ordering until the725C70 drain.
    detached_particles: BTreeMap<u64, DetachedParticle>,
    /// Which systems may have changed since [`Self::take_touched`]. One
    /// reader: the pointer-expiry listener index. Transient: never saved,
    /// compared or hashed.
    #[serde(skip)]
    touched: TouchLogs<1>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DetachedParticle {
    body: Particle,
    object_alive: bool,
    /// Object+81 is independent of Alive: common Limbo can precede UnInit.
    in_limbo: bool,
}

impl ParticleSystemStore {
    pub(crate) fn gas_cloud_system(&self) -> Option<u64> {
        self.gas_cloud_system
    }
    #[cfg(test)]
    pub fn new() -> Self {
        Self::default()
    }

    /// SystemPointerExpired62FE90: target+E4 only clears; owner+E0
    /// clears and ends emission, independently of the broadcast control byte.
    /// Whether the system changed.
    pub(crate) fn pointer_expired(&mut self, listener: u64, expired: u64) -> bool {
        let Some(sys) = self.get_mut(listener) else {
            return false;
        };
        let detached = sys.remove_expired_child(expired);
        let mut changed = detached.is_some();
        if sys.target_ref == Some(crate::sim::combat::TargetKind::Entity(expired)) {
            sys.target_ref = None;
            changed = true;
        }
        if sys.owner_entity == Some(expired) {
            sys.owner_entity = None;
            sys.done_spawning = true;
            changed = true;
        }
        if let Some(particle) = detached {
            // Removing a native pointer from +BC does not run a destructor.
            // Retain the uniquely owned body without inventing UnInit or
            // queueing work; both broadcast controls use this same callback.
            self.retain_detached_particle(particle);
        }
        changed
    }

    pub(crate) fn enter_limbo(&mut self, id: u64) {
        if let Some(sys) = self.get_mut(id) {
            sys.alive = false;
            sys.in_limbo = true;
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = (&u64, &ParticleSystem)> + '_ {
        self.systems.iter()
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&u64, &mut ParticleSystem)> + '_ {
        self.touched.note_all();
        self.systems.iter_mut()
    }

    /// Bodies eligible for an existing AIR Display entry. A detached live
    /// Particle remains allocated and drawable until its own Limbo/UnInit;
    /// dead queued bodies retain identity but contribute no drawing body.
    pub fn display_particles(&self) -> impl Iterator<Item = (Option<&ParticleSystem>, &Particle)> {
        self.systems
            .values()
            .flat_map(|system| {
                system
                    .particles
                    .iter()
                    .map(move |particle| (Some(system), particle))
            })
            .chain(
                self.detached_particles
                    .values()
                    .filter(|particle| particle.object_alive && !particle.in_limbo)
                    .map(|particle| (None, &particle.body)),
            )
    }

    pub fn get(&self, id: u64) -> Option<&ParticleSystem> {
        self.systems.get(&id)
    }

    pub fn get_mut(&mut self, id: u64) -> Option<&mut ParticleSystem> {
        let stored = self.systems.len();
        let sys = self.systems.get_mut(&id)?;
        self.touched.note(id, stored);
        Some(sys)
    }

    pub(crate) fn retain_detached_particle(&mut self, particle: Particle) -> u64 {
        let id = particle.stable_id();
        assert!(
            self.detached_particles
                .insert(
                    id,
                    DetachedParticle {
                        body: particle,
                        object_alive: true,
                        in_limbo: false,
                    },
                )
                .is_none(),
            "particle {id} detached twice"
        );
        id
    }

    pub(crate) fn finish_particle_retirement(&mut self, id: u64) {
        self.detached_particles
            .get_mut(&id)
            .expect("UnInit retains the particle through its callbacks")
            .object_alive = false;
    }

    /// Read Object+90 (Alive), then Object+81 (InLimbo) through this owner.
    pub(crate) fn detached_particle_lifecycle(&self, id: u64) -> Option<(bool, bool)> {
        self.detached_particles
            .get(&id)
            .map(|particle| (particle.object_alive, particle.in_limbo))
    }

    pub(crate) fn enter_particle_limbo(&mut self, id: u64) {
        self.detached_particles
            .get_mut(&id)
            .expect("Particle Limbo retains its body")
            .in_limbo = true;
    }

    pub(crate) fn detached_particle(&self, id: u64) -> Option<(&Particle, bool)> {
        self.detached_particles
            .get(&id)
            .map(|retired| (&retired.body, retired.object_alive))
    }

    /// Read-only identity/body view for hashing, snapshot validation and the
    /// common finalizer. The represented ordinary Particle has null base
    /// pointer-expiry fields, so it names no ids in the derived listener index.
    pub(crate) fn detached_particles(
        &self,
    ) -> impl ExactSizeIterator<Item = (u64, &Particle, bool, bool)> {
        self.detached_particles
            .iter()
            .map(|(&id, retired)| (id, &retired.body, retired.object_alive, retired.in_limbo))
    }

    pub(crate) fn finalize_particle(&mut self, id: u64) -> Option<Particle> {
        self.detached_particles
            .remove(&id)
            .map(|retired| retired.body)
    }

    /// Insert a system whose identity was assigned by the object substrate.
    pub(crate) fn insert(&mut self, sys: ParticleSystem) -> u64 {
        let id = sys.stable_id;
        debug_assert_ne!(id, 0, "particle system requires an assigned stable id");
        self.touched.note(id, self.systems.len());
        self.systems.insert(id, sys);
        id
    }

    /// Temporarily take a system while its AI owns `&mut Simulation`.
    pub(crate) fn take_for_tick(&mut self, id: u64) -> Option<ParticleSystem> {
        self.remove(id)
    }

    /// Reinsert a system after the temporary tick ownership round-trip.
    pub(crate) fn reinsert_after_tick(&mut self, sys: ParticleSystem) {
        let id = sys.stable_id;
        debug_assert!(id > 0, "reinsert requires a previously-assigned stable_id");
        self.touched.note(id, self.systems.len());
        self.systems.insert(id, sys);
    }

    /// Physical removal boundary used only by the shared pending-delete finalizer.
    pub(crate) fn finalize_remove(&mut self, id: u64) -> Option<ParticleSystem> {
        if self.gas_cloud_system == Some(id) {
            self.gas_cloud_system = None;
        }
        self.remove(id)
    }

    fn remove(&mut self, id: u64) -> Option<ParticleSystem> {
        let removed = self.systems.remove(&id);
        if removed.is_some() {
            self.touched.note(id, self.systems.len());
        }
        removed
    }

    /// Take the touch log, leaving it empty.
    pub(crate) fn take_touched(&mut self) -> Touched {
        self.touched.take(0)
    }

    pub(crate) fn contains_key(&self, id: u64) -> bool {
        self.systems.contains_key(&id)
    }

    pub fn len(&self) -> usize {
        self.systems.len()
    }

    pub fn is_empty(&self) -> bool {
        self.systems.is_empty()
    }
}

impl Simulation {
    /// Read-only access for presentation and deterministic state folding.
    pub fn particle_systems(&self) -> &ParticleSystemStore {
        &self.substrate.particle_systems
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_system(stable_id: u64) -> ParticleSystem {
        ParticleSystem::test_fixture(stable_id, ParticleSystemTypeId(0), IVec3::ZERO)
    }

    #[test]
    fn serde_roundtrip_preserves_authoritative_system_state() {
        let mut store = ParticleSystemStore::new();
        let mut system = fake_system(23);
        system.in_logic_vector = true;
        system.coords = IVec3::new(-17, 29, 43);
        system.target_coords = IVec3::new(101, -202, 303);
        system.particles.push({
            let mut particle = Particle::test_fixture(ParticleTypeId(4), IVec3::new(1, 2, 3));
            particle.origin = IVec3::new(7, 8, 9);
            particle.direction = [SimFixed::from_num(1); 3];
            particle.velocity = NativeF32Bits::from_bits(0x4000_0000);
            particle.lifetime_remaining = 31;
            particle.damage_counter = 5;
            particle.state_ai_advance = 2;
            particle.animation_state = 3;
            particle.translucency = 4;
            particle.drift_x = -1;
            particle.drift_y = 2;
            particle.drift_z = -3;
            particle.prev_delta = [SimFixed::from_num(4); 3];
            particle.stable_id = 24;
            particle.native_unique_id = -25;
            particle
        });
        store.insert(system);

        let bytes = crate::util::save_codec::serialize(&store).expect("serialize particle systems");
        let restored: ParticleSystemStore =
            crate::util::save_codec::deserialize(&bytes).expect("deserialize particle systems");
        let restored = restored.get(23).expect("system survives roundtrip");

        assert_eq!(restored.coords, IVec3::new(-17, 29, 43));
        assert!(!restored.in_logic_vector);
        assert_eq!(restored.target_coords, IVec3::new(101, -202, 303));
        assert_eq!(restored.particles.len(), 1);
        assert_eq!(restored.particles[0].origin, IVec3::new(7, 8, 9));
        assert_eq!(restored.particles[0].stable_id(), 24);
        assert_eq!(restored.particles[0].native_unique_id(), -25);
        assert_eq!(restored.particles[0].velocity().bits(), 0x4000_0000);
    }
}
