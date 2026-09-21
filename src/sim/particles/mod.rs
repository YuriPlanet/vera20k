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
//! Tier 2 implements Smoke / Gas / Fire via the existing SHP render pipeline.
//! Spark compatibility state and pure kernels exist, but public Spark/Railgun
//! spawn and production dispatch remain unavailable until their activation gates close.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on rules/ and util/ only.
//! - sim/ NEVER depends on render/, ui/, sidebar/, audio/, net/.

use crate::rules::particle_system_type::ParticleSystemTypeId;
use crate::rules::particle_type::ParticleTypeId;
use crate::sim::intern::InternedId;
use crate::sim::world::Simulation;
use crate::util::fixed_math::SimFixed;
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParticleSystem {
    pub stable_id: u64,
    /// LogicClass active-vector membership. The serialized vector is
    /// authoritative across load, so this object-local guard is rebuilt.
    #[serde(skip)]
    pub in_logic_vector: bool,
    pub type_id: ParticleSystemTypeId,
    #[serde(with = "ivec3_serde")]
    pub coords: IVec3,
    #[serde(with = "ivec3_serde")]
    pub offset: IVec3,
    pub particles: Vec<Particle>,
    pub spawn_timer: SimFixed,
    pub lifetime: i32,
    pub spark_spawn_frames: i32,
    pub facing: u8,
    pub directionless: bool,
    pub attached_entity: Option<u64>,
    pub owner_entity: Option<u64>,
    #[serde(with = "ivec3_serde")]
    pub target_coords: IVec3,
    pub owner_house: Option<InternedId>,
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
    pub done_spawning: bool,
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
    pub type_id: ParticleTypeId,
    #[serde(with = "ivec3_serde")]
    pub coords: IVec3,
    #[serde(with = "ivec3_serde")]
    pub previous_coords: IVec3,
    #[serde(with = "ivec3_serde")]
    pub origin: IVec3,
    pub direction: [SimFixed; 3],
    pub velocity: SimFixed,
    pub lifetime_remaining: i16,
    pub damage_counter: i16,
    pub state_ai_advance: u8,
    pub animation_state: u8,
    pub translucency: u8,
    pub hit_ground: bool,
    pub marked_for_deletion: bool,

    pub drift_x: i32,
    pub drift_y: i32,
    pub drift_z: i32,

    pub current_color: [u8; 3],
    pub color_index: u8,
    pub color_accumulator: SimFixed,

    /// Authoritative behavior-3 state. Generic direction/velocity/color fields
    /// remain authoritative for the existing Smoke/Gas/Fire implementations only.
    pub spark: Option<SparkRuntimeState>,

    /// Fire-only scratch: per-tick velocity delta computed by fire AI and
    /// consumed by `move_fire` (jitter * direction). Zero for smoke/gas.
    pub prev_delta: [SimFixed; 3],

    /// Per-particle sub-tick accumulator for the state-AI advance.
    /// Increments every tick; when it hits the per-type denominator
    /// `(image_frame_count % 2 + 1) + StateAIAdvance`, animation_state
    /// bumps by 1. Wraps at 256 (denom is always small in practice).
    pub state_advance_counter: u8,
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

impl ParticleSystem {
}

/// Deterministic store for `ParticleSystem` instances.
///
/// Mirrors `EntityStore`: BTreeMap-backed so storage iteration is deterministic.
/// Identity is assigned by `ObjectSubstrate`; this store deliberately has no
/// allocator of its own.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ParticleSystemStore {
    systems: BTreeMap<u64, ParticleSystem>,
}

impl ParticleSystemStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&u64, &ParticleSystem)> + '_ {
        self.systems.iter()
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&u64, &mut ParticleSystem)> + '_ {
        self.systems.iter_mut()
    }

    pub fn get(&self, id: u64) -> Option<&ParticleSystem> {
        self.systems.get(&id)
    }

    pub fn get_mut(&mut self, id: u64) -> Option<&mut ParticleSystem> {
        self.systems.get_mut(&id)
    }

    /// Insert a system whose identity was assigned by the object substrate.
    pub(crate) fn insert(&mut self, sys: ParticleSystem) -> u64 {
        let id = sys.stable_id;
        debug_assert_ne!(id, 0, "particle system requires an assigned stable id");
        self.systems.insert(id, sys);
        id
    }

    /// Temporarily take a system while its AI owns `&mut Simulation`.
    pub(crate) fn take_for_tick(&mut self, id: u64) -> Option<ParticleSystem> {
        self.systems.remove(&id)
    }

    /// Reinsert a system after the temporary tick ownership round-trip.
    pub(crate) fn reinsert_after_tick(&mut self, sys: ParticleSystem) {
        let id = sys.stable_id;
        debug_assert!(id > 0, "reinsert requires a previously-assigned stable_id");
        self.systems.insert(id, sys);
    }

    /// Physical removal boundary used only by the shared pending-delete finalizer.
    pub(crate) fn finalize_remove(&mut self, id: u64) -> Option<ParticleSystem> {
        self.systems.remove(&id)
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

    pub(crate) fn particle_systems_mut(&mut self) -> &mut ParticleSystemStore {
        &mut self.substrate.particle_systems
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_system(stable_id: u64) -> ParticleSystem {
        ParticleSystem {
            stable_id,
            in_logic_vector: false,
            type_id: ParticleSystemTypeId(0),
            coords: IVec3::ZERO,
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

    #[test]
    fn store_uses_preassigned_object_ids() {
        let mut store = ParticleSystemStore::new();
        assert_eq!(store.insert(fake_system(41)), 41);
        assert_eq!(store.insert(fake_system(97)), 97);
        assert!(store.get(41).is_some());
        assert!(store.get(97).is_some());
    }

    #[test]
    fn iteration_is_sorted_by_id() {
        let mut store = ParticleSystemStore::new();
        let _ = store.insert(fake_system(9));
        let _ = store.insert(fake_system(2));
        let _ = store.insert(fake_system(7));
        let ids: Vec<u64> = store.iter().map(|(id, _)| *id).collect();
        let mut sorted = ids.clone();
        sorted.sort();
        assert_eq!(ids, sorted);
    }

    #[test]
    fn tick_ownership_round_trip_preserves_id() {
        let mut store = ParticleSystemStore::new();
        let id = store.insert(fake_system(12));
        let sys = store.take_for_tick(id).unwrap();
        store.reinsert_after_tick(sys);
        assert!(store.get(id).is_some());
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn serde_roundtrip_preserves_authoritative_system_state() {
        let mut store = ParticleSystemStore::new();
        let mut system = fake_system(23);
        system.in_logic_vector = true;
        system.coords = IVec3::new(-17, 29, 43);
        system.target_coords = IVec3::new(101, -202, 303);
        system.particles.push(Particle {
            type_id: ParticleTypeId(4),
            coords: IVec3::new(1, 2, 3),
            previous_coords: IVec3::new(4, 5, 6),
            origin: IVec3::new(7, 8, 9),
            direction: [SimFixed::from_num(1); 3],
            velocity: SimFixed::from_num(2),
            lifetime_remaining: 31,
            damage_counter: 5,
            state_ai_advance: 2,
            animation_state: 3,
            translucency: 4,
            hit_ground: true,
            marked_for_deletion: false,
            drift_x: -1,
            drift_y: 2,
            drift_z: -3,
            current_color: [10, 20, 30],
            color_index: 2,
            color_accumulator: SimFixed::from_num(3),
            spark: None,
            prev_delta: [SimFixed::from_num(4); 3],
            state_advance_counter: 7,
        });
        store.insert(system);

        let bytes = bincode::serialize(&store).expect("serialize particle systems");
        let restored: ParticleSystemStore =
            bincode::deserialize(&bytes).expect("deserialize particle systems");
        let restored = restored.get(23).expect("system survives roundtrip");

        assert_eq!(restored.coords, IVec3::new(-17, 29, 43));
        assert!(!restored.in_logic_vector);
        assert_eq!(restored.target_coords, IVec3::new(101, -202, 303));
        assert_eq!(restored.particles.len(), 1);
        assert_eq!(restored.particles[0].origin, IVec3::new(7, 8, 9));
        assert_eq!(restored.particles[0].state_advance_counter, 7);
    }
}
