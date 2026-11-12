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
//! Smoke / Gas / Fire use the existing SHP render pipeline. Spark and Railgun
//! enter the shared live-object dispatch and draw as coloured points.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on rules/ and util/ only.
//! - sim/ NEVER depends on render/, ui/, audio/, net/.

use crate::rules::particle_system_type::ParticleSystemTypeId;
use crate::rules::particle_type::{ParticleBehavesLike, ParticleType, ParticleTypeId};
use crate::sim::intern::InternedId;
use crate::sim::touch_log::{TouchLogs, Touched};
use crate::sim::world::Simulation;
use crate::util::native_x87::{NativeF32Bits, NativeF64Bits};
use glam::IVec3;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub mod ares_system;
pub mod fire;
pub mod gas;
pub mod railgun;
pub mod smoke;
pub mod spark;
pub mod spark_spawn;
pub mod spark_world;
pub mod spawn;
pub mod system_ai;
pub mod wind;

#[cfg(all(test, feature = "ares"))]
mod ares_gas_tests;
#[cfg(test)]
mod fire_native_tests;
#[cfg(test)]
mod gas_native_tests;
#[cfg(test)]
mod native_fixture;
#[cfg(test)]
mod native_identity_tests;
// Ares simulates this Railgun system itself (hooks 0x62DF05, 0x62FD60).
#[cfg(all(test, not(feature = "ares")))]
mod railgun_native_tests;
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
    /// FireAt stored this system in its owner's `+0x304` (`0x006FF1A7`). The
    /// owner's releases clear that slot; [`Self::apply_fire_link_release`]
    /// gives the system the done byte their vt+0xF8 call sets.
    #[serde(default)]
    owner_fire_link: bool,
    alive: bool,
    in_limbo: bool,
    /// Ares 3.0 ParticleSystemExt (`ares_system`): present for a system Ares
    /// simulates itself, from the constructor hook `0x62DF05` on.
    #[serde(default)]
    ares: Option<Box<ares_system::AresParticleSystemExt>>,
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
    /// Constructor62B674..62B69F retains both endpoints; GetDrawFrame62D830
    /// uses them even after the parent system has retired.
    #[serde(with = "ivec3_serde")]
    target: IVec3,
    /// Constructor62B707..62B71B stores the original (unclamped) source as
    /// floats for every particle. AI_Railgun62C3A0 advances these words.
    float_position: [NativeF32Bits; 3],
    /// Shared +B0/+B4/+B8 colour state, initialized by Constructor62B5E0
    /// for every behavior and consumed by Spark/Railgun AI and drawing.
    start_rgb: [u8; 3],
    color_index: i32,
    color_accumulator: NativeF64Bits,
    /// `+0x10C`: the constructor's normalized target vector, which Fire's AI
    /// scales into its step and AI_Railgun replaces with its drift vector.
    /// It is distinct from the Gas motion words.
    direction: [NativeF32Bits; 3],
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

    /// `+0x100`: Fire's step, written by its AI (`0x0062CBB0`) and added to
    /// Location by its move (`0x0062D64E`). Zero for every other behavior.
    prev_delta: [i32; 3],
}

impl Particle {
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_stable_id_for_test(&mut self, value: u64) {
        self.stable_id = value;
    }

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
    pub fn direction_mut_for_test(&mut self) -> &mut [NativeF32Bits; 3] {
        &mut self.direction
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn motion_mut_for_test(&mut self) -> &mut [NativeF32Bits; 3] {
        &mut self.motion
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn prev_delta_mut_for_test(&mut self) -> &mut [i32; 3] {
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
    pub fn set_color_for_test(&mut self, rgb: [u8; 3], index: i32, accumulator: NativeF64Bits) {
        self.start_rgb = rgb;
        self.color_index = index;
        self.color_accumulator = accumulator;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn float_position_mut_for_test(&mut self) -> &mut [NativeF32Bits; 3] {
        &mut self.float_position
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_target_for_test(&mut self, target: IVec3) {
        self.target = target;
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

    pub const fn direction(&self) -> [NativeF32Bits; 3] {
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

    pub const fn target(&self) -> IVec3 {
        self.target
    }
    pub const fn float_position(&self) -> [NativeF32Bits; 3] {
        self.float_position
    }
    pub const fn start_rgb(&self) -> [u8; 3] {
        self.start_rgb
    }
    pub const fn color_index(&self) -> i32 {
        self.color_index
    }
    pub const fn color_accumulator(&self) -> NativeF64Bits {
        self.color_accumulator
    }

    /// ParticleClass::GetDrawFrame @ 0x0062D830: Fire projects each saved
    /// endpoint separately through6D2140, then75F230. Both animation bytes
    /// are sign-extended; unknown behavior returns frame zero.
    pub fn draw_frame(&self, kind: &ParticleType) -> i32 {
        let state = i32::from(self.animation_state as i8);
        match kind.behaves_like {
            ParticleBehavesLike::Gas | ParticleBehavesLike::Smoke => state,
            ParticleBehavesLike::Fire => {
                let project = |point: IVec3| {
                    let (x, y) = crate::util::lepton::project_absolute_lepton_xy(point.x, point.y);
                    (
                        x,
                        y.wrapping_sub(crate::util::native_x87::adjust_for_z_standard(point.z)),
                    )
                };
                let octant = crate::util::direction::direction_from_projected(
                    project(self.origin),
                    project(self.target),
                );
                state + (octant & 3) * i32::from(kind.end_state_ai as i8)
            }
            ParticleBehavesLike::Unknown
            | ParticleBehavesLike::Spark
            | ParticleBehavesLike::Railgun => 0,
        }
    }

    pub const fn prev_delta(&self) -> [i32; 3] {
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
        Simulation::reveal_particle(&mut sim.substrate, particle.stable_id);
        particle
    }

    /// Constructor62B5E0 terrain seam used by the existing Spark controls,
    /// including the colour seed (`0x0062BA51`) for any type with a
    /// `ColorList`. Spark registers only after that colour initialization.
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
        // The native Particle constructor (0x0062B5E0) divides its lifetime
        // draw by MaxEC, so `MaxEC=0`, which only a map or mod can set, traps
        // in gamemd. VERA carries on: no draw and no extra lifetime. The
        // identity and any Smoke draw are already taken here and the callers
        // cannot carry a refusal, so the particle is still built.
        let lifetime_extra = if divisor == 0 {
            log::warn!("particle type {type_id:?} has MaxEC=0: no lifetime draw");
            0
        } else {
            sim.particle_rng().next_raw_abs_modulo(divisor) as i16
        };
        let lifetime_remaining = (pt.max_ec as i16).wrapping_add(lifetime_extra);
        let location = floor_constructor_coords_with(coords, |x, y| ground(sim, x, y))?;
        let direction = spawn::native_velocity(coords, target);
        let advance = spawn::spawn_state_ai_advance(pt, coords, target, direction, velocity);
        let start_rgb = spark_spawn::constructor_start_color(sim, pt)
            .expect("a ranged draw scales to a finite colour fraction");
        let particle = Particle {
            stable_id,
            native_unique_id,
            type_id,
            coords: location,
            origin: coords,
            target,
            float_position: coords.to_array().map(|axis| {
                use crate::util::native_x87::MaskedX87Chop53 as Fpu;
                Fpu::store_f32_masked_chop(Fpu::load_i32(axis))
            }),
            start_rgb,
            color_index: 0,
            color_accumulator: NativeF64Bits::POSITIVE_ZERO,
            direction,
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
            prev_delta: [0; 3],
        };
        Ok(particle)
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

    /// The Ares records of a system Ares simulates ([`ares_system`]).
    pub fn ares(&self) -> Option<&ares_system::AresParticleSystemExt> {
        self.ares.as_deref()
    }

    /// No ParticleClass child (`+0xCC`) and, under Ares 3.0
    /// (`ExtData::Update @ 10026C70`), no record of its own either: the
    /// emptiness the AI tail retires a done system on.
    pub(crate) fn holds_no_particles(&self) -> bool {
        if self.ares.as_ref().is_some_and(|ext| !ext.is_empty()) {
            return false;
        }
        self.particles.is_empty()
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

    pub const fn owner_fire_link(&self) -> bool {
        self.owner_fire_link
    }

    /// The owner's release of `+0x304` (Foot `0x004D954B`, Assign_Target
    /// `0x006FCF78`, ReceiveDamage `0x00702220`) calls this system's vt+0xF8,
    /// which only sets the done byte (`0x006301E0`), and clears the slot. Only
    /// this system's AI reads that byte, whatever its behavior, so the system
    /// sets it here, before each of those reads, once its owner no longer
    /// holds it. An owner gone altogether also ends spawning: AI_Fire's owner
    /// test sets the byte too.
    pub(crate) fn apply_fire_link_release(
        &mut self,
        entities: &crate::sim::entity_store::EntityStore,
    ) {
        if !self.owner_fire_link {
            return;
        }
        let held = self
            .owner_entity
            .and_then(|owner| entities.get(owner))
            .is_some_and(|owner| owner.fire_particle_system.system() == Some(self.stable_id));
        if !held {
            self.owner_fire_link = false;
            self.done_spawning = true;
        }
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
            owner_fire_link: false,
            alive: true,
            in_limbo: false,
            ares: None,
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
            target: coords,
            float_position: coords.to_array().map(|axis| {
                use crate::util::native_x87::MaskedX87Chop53 as Fpu;
                Fpu::store_f32_masked_chop(Fpu::load_i32(axis))
            }),
            start_rgb: [0; 3],
            color_index: 0,
            color_accumulator: NativeF64Bits::POSITIVE_ZERO,
            direction: [NativeF32Bits::POSITIVE_ZERO; 3],
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
            prev_delta: [0; 3],
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
    /// Which systems may have changed since a reader last took its log
    /// ([`Self::take_touched`]). Transient: never saved, compared or hashed.
    #[serde(skip)]
    touched: TouchLogs<{ ParticleTouchReader::COUNT }>,
}

/// A product derived from the particle systems that re-derives from the touch
/// log. Each reads its own log, so one reader's take leaves the others' intact.
#[derive(Debug, Clone, Copy)]
pub(crate) enum ParticleTouchReader {
    /// The kept Ground display sort keys.
    GroundKeys = 0,
    /// The pointer-expiry broadcast's listener index.
    ExpiryListeners = 1,
}

impl ParticleTouchReader {
    const COUNT: usize = 2;
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

    /// FireAt's `+0x304` store (`0x006FF1A7`) of the system it just built.
    pub(crate) fn link_to_owner_fire_slot(&mut self, id: u64) {
        if let Some(system) = self.get_mut(id) {
            system.owner_fire_link = true;
        }
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

    /// Every particle: the detached ones, then each system's.
    pub(crate) fn all_particles(&self) -> impl Iterator<Item = &Particle> {
        self.detached_particles
            .values()
            .map(|retired| &retired.body)
            .chain(
                self.systems
                    .values()
                    .flat_map(|system| system.particles.iter()),
            )
    }

    /// The particle `id`, detached or still in its system. Systems keep no
    /// index of their particles, so this walks them.
    pub(crate) fn particle(&self, id: u64) -> Option<&Particle> {
        self.detached_particles
            .get(&id)
            .map(|retired| &retired.body)
            .or_else(|| {
                self.systems
                    .values()
                    .flat_map(|system| system.particles.iter())
                    .find(|particle| particle.stable_id == id)
            })
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

    /// Resolve a child across synchronous callbacks which may append or compact
    /// its system. The ordinary store keeps the system's display identity live.
    pub(crate) fn particle_for_tick(&self, system: u64, index: usize, id: u64) -> Option<Particle> {
        let particles = &self.get(system)?.particles;
        particles
            .get(index)
            .filter(|p| p.stable_id() == id)
            .or_else(|| particles.iter().find(|p| p.stable_id() == id))
            .cloned()
    }

    /// Commit only the original child, preserving callback appends/removals.
    pub(crate) fn commit_particle_tick(&mut self, system: u64, index: usize, updated: Particle) {
        if let Some(sys) = self.get_mut(system) {
            let slot = if sys
                .particles
                .get(index)
                .is_some_and(|p| p.stable_id() == updated.stable_id())
            {
                Some(index)
            } else {
                sys.particles
                    .iter()
                    .position(|p| p.stable_id() == updated.stable_id())
            };
            if let Some(slot) = slot {
                sys.particles[slot] = updated;
            }
        }
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

    /// Take `reader`'s touch log, leaving it empty.
    pub(crate) fn take_touched(&mut self, reader: ParticleTouchReader) -> Touched {
        self.touched.take(reader as usize)
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
    fn zero_max_ec_constructs_without_a_lifetime_draw() {
        let rules =
            crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
                "[Particles]\n0=GasP\n[GasP]\nBehavesLike=Gas\nMaxEC=0\n",
            ))
            .expect("particle rules");
        let mut sim = Simulation::with_seed(7);
        let rng_before = sim.scenario_rng.logical_state();
        let particle = Particle::new(
            ParticleTypeId(0),
            IVec3::ZERO,
            IVec3::ZERO,
            rules.particle_type(ParticleTypeId(0)),
            &mut sim,
        );
        assert_eq!(particle.lifetime_remaining(), 0);
        assert_eq!(sim.scenario_rng.logical_state(), rng_before);
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
            particle.direction = [NativeF32Bits::from_bits(0x3f80_0000); 3];
            particle.velocity = NativeF32Bits::from_bits(0x4000_0000);
            particle.lifetime_remaining = 31;
            particle.damage_counter = 5;
            particle.state_ai_advance = 2;
            particle.animation_state = 3;
            particle.translucency = 4;
            particle.drift_x = -1;
            particle.drift_y = 2;
            particle.drift_z = -3;
            particle.prev_delta = [4; 3];
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
