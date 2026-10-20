//! Particle spawn helpers.
//!
//! Three entry points:
//!   - `Simulation::spawn_particle_system` — public API for combat / refinery /
//!     gap-gen / area-damage to create a new system at a world coord.
//!   - `spawn_particle` — append one particle to a system's vector, capped by
//!     `ParticleSystemType::particle_cap`. Used by per-tick system AI.
//!   - `spawn_particle_with_insert` — Fire-only variant: append, then random-
//!     shuffle within the last `insert_range` slots so the visual stream has
//!     variety instead of strict FIFO.
//!
//! Railgun remains refused by the public entry. Spark systems enter the
//! shared live Logic vector and their owned particles run in that system visit.

use super::{Particle, ParticleSystem};
use crate::rules::particle_system_type::{ParticleSystemBehavesLike, ParticleSystemTypeId};
use crate::rules::ruleset::RuleSet;
use crate::sim::intern::InternedId;
use crate::sim::world::Simulation;
#[cfg(any(test, feature = "test-support"))]
use crate::util::fixed_math::SimFixed;
use crate::util::native_x87::{
    NativeF32Bits, NativeF64Bits, X87Chop53, X87Ordering, sqrt_approx_f32,
};
use glam::IVec3;

/// TechnoClass+308: the retained Spark ParticleSystem, shared by FireAt6FF1F5
/// and the Cyborg AI producer6FAEDA. Constructor6F3231 clears it;
/// PointerExpired707AC2..707AD7 clears an exact match for either control byte.
/// The system's actual Logic/child lifetime owns expiry, not a Techno timer.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub(crate) struct AttachedSpark {
    system: Option<u64>,
}

impl AttachedSpark {
    pub(crate) const fn system(self) -> Option<u64> {
        self.system
    }

    /// The native producer stores the constructor result, including NULL.
    pub(crate) fn bind(&mut self, system: Option<u64>) {
        self.system = system;
    }

    pub(crate) fn forget(&mut self) {
        self.system = None;
    }
}

/// The object's damage-Smoke `ParticleSystemClass` (`TechnoClass+0x310`).
/// The surviving ReceiveDamage postlude creates the system synchronously and
/// records it here (`Simulation::maintain_damage_smoke_after_receive`); it
/// UnInits the system when health recovers above `ConditionYellow`, and
/// `TechnoClass::PointerExpired` drops the handle when the system expires
/// ([`Self::forget`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct DamageSmoke {
    system: Option<u64>,
}

impl DamageSmoke {
    /// The live system's stable id.
    pub(crate) const fn system(self) -> Option<u64> {
        self.system
    }

    /// PointerExpired's drop of the expiring system.
    pub(crate) fn forget(&mut self) {
        self.system = None;
    }

    /// Install a fixture's system.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_for_test(&mut self, system: Option<u64>) {
        self.system = system;
    }
}

impl Simulation {
    /// Cyborg-only TechnoAI6FACD9..6FAEDA. Stock YR leaves Cyborg unset, so
    /// ordinary damaged vehicles never enter this producer. Both it and
    /// FireAt use the actual +308 link and the one system constructor.
    pub(crate) fn update_damage_spark_after_ai(&mut self, id: u64, rules: &RuleSet) {
        use crate::util::native_x87::{MaskedX87Chop53 as X87, MaskedX87Ordering, NativeF64Bits};

        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        let Some(object) = self.object_type(entity.type_ref(), rules) else {
            return;
        };
        if !object.emits_damage_spark() {
            return;
        }
        let ratio = entity.health.ratio(object.strength);
        let below = |threshold: f64| {
            matches!(
                X87::compare(
                    ratio,
                    X87::load_f64(NativeF64Bits::from_bits(threshold.to_bits()))
                ),
                MaskedX87Ordering::Less | MaskedX87Ordering::Unordered
            )
        };
        if !below(rules.general.condition_yellow)
            || self.damage_particle_owner_height(entity) <= -10
            || entity.spark_particle_system.system().is_some()
        {
            return;
        }
        // 6FAD51..6FADA1 retains Spark entries in authored list order.
        let systems: Vec<_> = object
            .damage_particle_systems
            .iter()
            .filter_map(|name| rules.ps_type_id_by_name(name))
            .filter(|&type_id| {
                rules.particle_system_type(type_id).behaves_like == ParticleSystemBehavesLike::Spark
            })
            .collect();
        if systems.is_empty() {
            return;
        }
        let threshold = if below(rules.general.condition_red) {
            rules.general.condition_red_spark_threshold
        } else {
            rules.general.condition_yellow_spark_threshold
        };
        let offset = damage_particle_offset(object);
        let origin = crate::sim::movement::ground_pose::object_get_coords(
            entity,
            self.resolved_terrain.as_ref(),
        );
        let coords = IVec3::new(
            origin.x.wrapping_add(offset.x),
            origin.y.wrapping_add(offset.y),
            origin.z.wrapping_add(offset.z),
        );
        // 6FAE11..6FAE40: Scenario ranged probability, then an admitted
        // list selection. The shared RNG owner handles equal endpoints.
        if self.scenario_rng.next_range_u32_inclusive(0, 0x7fff_fffe) >= threshold {
            return;
        }
        let selected = self
            .scenario_rng
            .next_range_u32_inclusive(0, systems.len() as u32 - 1);
        // 6FAEBC..6FAED1: supplied target NULL, owner=this, zero fifth
        // Coord (B0EA90 initialized by6F2A50), owner House NULL. Native
        // constructor target/owner derivation remains centralized below.
        let system = self.spawn_particle_system(
            systems[selected as usize],
            coords,
            None,
            Some(id),
            IVec3::ZERO,
            None,
            rules,
        );
        if let Some(entity) = self.substrate.entities.get_mut(id) {
            entity.spark_particle_system.bind(system);
        }
    }

    /// Mark the attached Techno+310 smoke system for retirement while retaining
    /// its owner pointer. Native ParticleSystem virtual F8 (6301E0) only sets
    /// its done-spawning byte; the done-empty Limbo broadcast owns pointer expiry.
    pub(crate) fn retire_attached_damage_smoke(&mut self, stable_id: u64) {
        let system_id = self
            .substrate
            .entities
            .get(stable_id)
            .and_then(|entity| entity.damage_smoke_system_id.system());
        if let Some(system_id) = system_id
            && let Some(system) = self.particle_systems_mut().get_mut(system_id)
        {
            system.mark_done_spawning();
        }
    }

    /// Reached healing tails 6FA75A,45096C,451477: ordered Greater only.
    /// This never creates smoke and never clears the retained owner slot.
    pub(crate) fn retire_damage_smoke_after_heal(&mut self, stable_id: u64, rules: &RuleSet) {
        let above = self
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| {
                self.object_type(entity.type_ref(), rules)
                    .is_some_and(|object| {
                        entity
                            .health
                            .compare_ratio(object.strength, rules.general.condition_yellow)
                            == crate::util::native_x87::MaskedX87Ordering::Greater
                    })
            });
        if above {
            self.retire_attached_damage_smoke(stable_id);
        }
    }

    /// Selfheal6FA75A has an additional Object GetHeight5F5F40 arm after a
    /// non-Greater health comparison. Read retained raw Object XYZ (not the
    /// building's foundation-center coordinate), then the live terrain surface.
    pub(crate) fn retire_damage_smoke_after_self_heal(&mut self, stable_id: u64, rules: &RuleSet) {
        let retire = self
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| {
                let Some(object) = self.object_type(entity.type_ref(), rules) else {
                    return false;
                };
                if entity
                    .health
                    .compare_ratio(object.strength, rules.general.condition_yellow)
                    == crate::util::native_x87::MaskedX87Ordering::Greater
                {
                    return true;
                }
                self.damage_particle_owner_height(entity) < -10
            });
        if retire {
            self.retire_attached_damage_smoke(stable_id);
        }
    }
    /// Shared original Object5F5F40 adapter for the Smoke and Cyborg Spark gates.
    fn damage_particle_owner_height(&self, entity: &crate::sim::game_entity::GameEntity) -> i32 {
        crate::sim::movement::air_movement::current_fly_height(
            entity,
            self.resolved_terrain.as_ref(),
        )
    }

    /// Spawn a new particle system. Returns the new system's stable id, or
    /// `None` for a `Railgun` type, which is still unimplemented.
    ///
    /// gamemd-derived: `ParticleSystemClass::Constructor @ 0x0062DC50`. Spark
    /// systems are admitted and simulated — `particles/spark_spawn.rs` owns
    /// `ParticleSystemClass::AI_Spark @ 0x0062E840`'s burst half and
    /// `particles/spark.rs` the per-particle kernel. Railgun is still refused.
    ///
    /// The shared `combat::electric_bolt::create` owner creates one
    /// `[CombatDamage] DefaultSparkSystem` at the bolt's target endpoint,
    /// matching `EBolt::Init @ 0x004C2A60`'s construction at `0x004C2B30`:
    /// no owner house, no attachment, handle discarded, no constructor RNG.
    /// Both system and particle constructors retain their AbstractClass IDs.
    /// Ordinary FireAt and SpawnShrapnel call this same owner. The latter
    /// reaches it once per child whose weapon has `IsElectricBolt`, including
    /// retail Electricbounce -> TeslaFragment on elite Tesla discharges.
    ///
    /// FireAt6FF1EC uses this constructor for UseSparkParticles and retains
    /// its result in the same Techno+308 owner as the Cyborg AI producer.
    /// Other Spark producers are wired through combat/parasite.rs
    /// (62A103), capture_manager.rs (471C15), and powered_unit.rs (7361A4).
    ///
    /// Stock-dormant, and recorded so it is not enabled for vehicles: the
    /// `DamageParticleSystems=` Spark producer in
    /// `TechnoClass::AI_Update @ 0x006FADB3` is Tiberian Sun legacy and never
    /// runs in stock YR, despite 126 of the 141 stock `DamageParticleSystems=`
    /// entries naming a Spark system. Its top gate at `0x006FACD9` reads
    /// `TechnoType+0xC8F`, whose only setter is `0x005243E7` in
    /// `InfantryTypeClass::ReadINI`, conditional on the section's `Cyborg=`
    /// bool — and no stock INI authors `Cyborg=` (`rulesmd.ini:3581` mentions
    /// it only in a TS-inherited comment). Enabling it for vehicles would add
    /// sparks retail does not show and consume `ScenarioClass::Random` draws
    /// gamemd does not take, which desyncs. The `Smoke` producer below is a
    /// different function (`TechnoClass::ReceiveDamage @ 0x00701900`) and is
    /// not affected.
    pub fn spawn_particle_system(
        &mut self,
        type_id: ParticleSystemTypeId,
        coords: IVec3,
        target: Option<crate::sim::combat::TargetKind>,
        owner_entity: Option<u64>,
        target_coords: IVec3,
        owner_house: Option<InternedId>,
        rules: &RuleSet,
    ) -> Option<u64> {
        let pst = rules.particle_system_type(type_id);
        if pst.behaves_like == ParticleSystemBehavesLike::Railgun {
            log::warn!(
                "particles: Tier 3 PSC type {:?} requested at {:?} — skipped",
                pst.behaves_like,
                coords,
            );
            return None;
        }
        // Original62DC50 calls Abstract410230 -> Scenario68BCB0 before
        // registration. The native numeric identity is not the Rust handle.
        // Executed constructor/CRC: procedural_drawing_oracle/electric_bolt.
        let native_unique_id = self.next_native_runtime_id();
        let stable_id = self.allocate_stable_id();
        // 62DD90..62DE14: a supplied target provides GetCoords; a Cell on
        // a structural bridge additionally receives the native bridge height.
        // The existing target-coordinate owner supplies that same Cell result.
        // Only a NULL target copies the fifth constructor Coord argument.
        let target_coords = target.map_or(Some(target_coords), |target| {
            crate::sim::movement::ground_pose::target_get_target_coords(
                target,
                self,
                self.resolved_terrain.as_ref(),
            )
            .map(|coord| IVec3::new(coord.x, coord.y, coord.z))
        })?;
        // 62DE35..62DE5A: the target pointer and its frozen coordinate are
        // distinct. An owner Techno contributes its current TarCom; without
        // an owner the constructor retains the supplied target itself.
        let target_ref = owner_entity.map_or(target, |owner| {
            self.substrate
                .entities
                .get(owner)
                .and_then(|owner| owner.attack_target.target())
        });
        // Constructor62DE6D..62DEAF: retain Location - owner GetCoords,
        // not the current type's DamageSmokeOffset. Smoke62ED49 consumes
        // this stored world-lepton displacement; other behaviors retain it.
        // Source/control: tools/spatial_oracle/damage_smoke.md (--following).
        let offset = owner_entity
            .and_then(|id| {
                crate::sim::movement::ground_pose::target_get_coords(
                    crate::sim::combat::TargetKind::Entity(id),
                    self,
                    self.resolved_terrain
                        .as_ref()
                        .map(crate::map::resolved_terrain::NativeCellQuery::canonical)
                        .as_ref(),
                )
            })
            .map_or(IVec3::ZERO, |origin| {
                IVec3::new(
                    coords.x.wrapping_sub(origin.x),
                    coords.y.wrapping_sub(origin.y),
                    coords.z.wrapping_sub(origin.z),
                )
            });
        let sys = ParticleSystem {
            stable_id,
            native_unique_id,
            in_logic_vector: false,
            type_id,
            coords,
            offset,
            particles: Vec::new(),
            spawn_timer: X87Chop53::store_f32(X87Chop53::load_i32(pst.spawn_frames))
                .expect("signed SpawnFrames fits stored binary32"),
            lifetime: pst.lifetime,
            spark_spawn_frames: pst.spark_spawn_frames as i32,
            facing: 0x1D,
            target_ref,
            owner_entity,
            target_coords,
            owner_house,
            done_spawning: false,
            alive: true,
            in_limbo: false,
        };
        debug_assert!(
            !self.substrate.entities.contains(stable_id)
                && !self.substrate.anims.contains_key(stable_id)
                && !self.particle_systems().contains_key(stable_id),
            "shared object id {stable_id} collided before particle-system insertion"
        );
        self.particle_systems_mut().insert(sys);
        self.reveal_particle_system(stable_id, Some(rules));
        Some(stable_id)
    }

    /// Execute88 at6DF3F7..6DF49B: signed waypoint center642740, then
    /// Ground578080 (no bridge lookup), then the existing62DC50 constructor.
    /// Target/owner/House are NULL; target Coord B0E6A0 is zero from6DCF20.
    /// Original Execute ignores constructor return and returns AL1.
    pub(crate) fn trigger_create_particle_system_at_waypoint(
        &mut self,
        index: i32,
        waypoint: i32,
        rules: &RuleSet,
    ) -> bool {
        let Some(type_id) = rules.particle_system_type_id_by_index(index) else {
            // Native indexes unchecked; malformed Rust inputs fail safely.
            return false;
        };
        let Some(cell) = self.session.waypoint_cell(waypoint) else {
            return false;
        };
        let Some(terrain) = self.resolved_terrain.as_ref() else {
            return false;
        };
        let center = crate::sim::cell_kernel::cell_center(
            crate::sim::cell_kernel::CellCoordinate {
                x: i32::from(cell.0),
                y: i32::from(cell.1),
            },
            0,
        );
        let cells = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain);
        let Ok(z) = crate::sim::movement::ground_pose::query_ground_height(
            &cells,
            crate::sim::components::DriveCoord {
                x: center.x,
                y: center.y,
                z: 0,
            },
        ) else {
            return false;
        };
        self.spawn_particle_system(
            type_id,
            IVec3::new(center.x, center.y, z),
            None,
            None,
            IVec3::ZERO,
            None,
            rules,
        );
        true
    }

    /// Execute89 at6DF49E..6DF518: all Alive systems whose GetCell41BEA0
    /// equals the waypoint receive virtual F8/6301E0. No type/House filter,
    /// RNG, timer, detach or physical deletion; even an empty list returns1.
    pub(crate) fn trigger_mark_particle_systems_done_at_waypoint(&mut self, waypoint: i32) -> bool {
        let Some(cell) = self.session.waypoint_cell(waypoint) else {
            return false;
        };
        for (_, system) in self.particle_systems_mut().iter_mut() {
            if !system.alive() {
                continue;
            }
            let at = (
                crate::util::lepton::lepton_to_cell_packed(system.coords.x),
                crate::util::lepton::lepton_to_cell_packed(system.coords.y),
            );
            if at == cell {
                system.mark_done_spawning();
            }
        }
        true
    }

    /// Maintain TechnoClass's attached damage-Smoke slot from the surviving
    /// ReceiveDamage postlude (`TechnoClass +0x310`). This runs synchronously
    /// before the receiver may retaliate or return to Infantry scatter.
    pub(crate) fn maintain_damage_smoke_after_receive(
        &mut self,
        stable_id: u64,
        state: crate::sim::combat::damage::DamageState,
        rules: &RuleSet,
    ) {
        let Some(entity) = self.substrate.entities.get(stable_id) else {
            return;
        };
        let Some(object) = self.object_type(entity.type_ref(), rules) else {
            return;
        };
        let above_yellow = entity
            .health
            .compare_ratio(object.strength, rules.general.condition_yellow)
            == crate::util::native_x87::MaskedX87Ordering::Greater;
        let current_system = entity.damage_smoke_system_id.system();

        if above_yellow {
            self.retire_attached_damage_smoke(stable_id);
            return;
        }

        if current_system.is_some()
            || !matches!(
                state,
                crate::sim::combat::damage::DamageState::Yellow
                    | crate::sim::combat::damage::DamageState::Red
            )
        {
            return;
        }

        let Some((coords, owner_entity, system_types)) =
            self.substrate.entities.get(stable_id).and_then(|entity| {
                let object = self.interner.object_type(entity.type_ref(), rules)?;
                let offset = damage_particle_offset(object);
                let raw = crate::sim::movement::ground_pose::position_world_coord(&entity.position);
                let coords = IVec3::new(
                    raw.x.wrapping_add(offset.x),
                    raw.y.wrapping_add(offset.y),
                    raw.z.wrapping_add(offset.z),
                );
                let smoke = object
                    .damage_particle_systems
                    .iter()
                    .rev()
                    .filter_map(|name| rules.ps_type_id_by_name(name))
                    .filter(|&id| {
                        rules.particle_system_type(id).behaves_like
                            == ParticleSystemBehavesLike::Smoke
                    })
                    .collect::<Vec<_>>();
                Some((coords, stable_id, smoke))
            })
        else {
            return;
        };
        if system_types.is_empty() {
            return;
        }

        // Original702952..70295F queries height only after the existing-slot,
        // damage-result and nonempty-type-list gates. Strictly greater than-10;
        // a rejected height must consume no selection RNG or constructor ID.
        if self
            .substrate
            .entities
            .get(stable_id)
            .is_none_or(|entity| self.damage_particle_owner_height(entity) <= -10)
        {
            return;
        }
        let selected = self
            .scenario_rng
            .next_range_u32_inclusive(0, system_types.len().saturating_sub(1) as u32)
            as usize;
        let Some(system_id) = self.spawn_particle_system(
            system_types[selected],
            coords,
            None,
            Some(owner_entity),
            IVec3::ZERO,
            None,
            rules,
        ) else {
            return;
        };
        if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
            entity.damage_smoke_system_id.system = Some(system_id);
        }
    }
}

/// TechnoTypeClass::GetDamageParticleOffset @0x007178C0. The screen-relative
/// arm first runs the retail isometric-pixel matrix, then folds authored Z
/// into all three world axes through Sqrt_Approx/ftol.
fn damage_particle_offset(object: &crate::rules::object_type::ObjectType) -> IVec3 {
    let offset = object.damage_smoke_offset;
    if !object.dam_smk_off_scrn_rel {
        return offset;
    }

    let a = f32::from_bits(0x4088_88CE);
    let px = offset.x as f32;
    let py = offset.y as f32;
    let iso_x = (a * (px + 2.0 * py)) as i32;
    let iso_y = (a * (-px + 2.0 * py)) as i32;

    let z_times_ten = offset.z.wrapping_mul(10);
    let z = X87Chop53::load_i32(z_times_ten);
    let squared_twice = X87Chop53::add(X87Chop53::mul(z, z), X87Chop53::mul(z, z));
    let root_bits = sqrt_approx_f32(squared_twice)
        .expect("damage-smoke offset square stays finite in authored coordinate range");
    let root = X87Chop53::load_f32(root_bits)
        .expect("Sqrt_Approx damage-smoke offset is finite normal or zero");
    let magnitude = X87Chop53::ftol_i64(root)
        .expect("damage-smoke offset magnitude fits a signed integer") as i32;
    let vertical = if offset.z >= 0 { -magnitude } else { magnitude };

    IVec3::new(
        iso_x.wrapping_sub(z_times_ten),
        iso_y.wrapping_sub(z_times_ten),
        vertical,
    )
}

/// Append one particle to `sys.particles`. Returns `false` when the system's
/// type has no `HoldsWhat` set or its particle cap is already reached.
pub(super) fn spawn_particle(
    sys: &mut ParticleSystem,
    coords: IVec3,
    rules: &RuleSet,
    sim: &mut Simulation,
) -> bool {
    let pst = rules.particle_system_type(sys.type_id);
    let Some(pt_id) = pst.holds_what else {
        return false;
    };
    if sys.particles.len() >= pst.particle_cap as usize {
        return false;
    }
    let pt = rules.particle_type(pt_id);
    let particle = Particle::new(pt_id, coords, sys.target_coords, pt, sim);
    sys.particles.push(particle);
    true
}

/// ParticleClass's constructor velocity: target less source, each component
/// stored as binary32 (`0x0062B6C6..0x0062B707`), then normalized in place
/// (`0x0062B8D7..0x0062B956`): `(x² + y²) + z²` through `Sqrt_Approx`, each
/// component divided by that length and stored as binary32. A zero length
/// leaves the vector as it is.
pub(super) fn native_velocity(source: IVec3, target: IVec3) -> [NativeF32Bits; 3] {
    let velocity = [
        target.x.wrapping_sub(source.x),
        target.y.wrapping_sub(source.y),
        target.z.wrapping_sub(source.z),
    ]
    .map(|delta| {
        X87Chop53::store_f32(X87Chop53::load_i32(delta))
            .expect("a coordinate difference is a finite binary32")
    });
    let [x, y, z] = velocity
        .map(|component| X87Chop53::load_f32(component).expect("a stored binary32 is finite"));
    let squared = X87Chop53::add(
        X87Chop53::add(X87Chop53::mul(x, x), X87Chop53::mul(y, y)),
        X87Chop53::mul(z, z),
    );
    let length = sqrt_approx_f32(squared).expect("a sum of binary32 squares is finite");
    if length.bits() == 0 {
        return velocity;
    }
    let length = X87Chop53::load_f32(length).expect("Sqrt_Approx returns a finite binary32");
    [x, y, z].map(|component| {
        X87Chop53::div(component, length)
            .and_then(X87Chop53::store_f32)
            .expect("a component over a nonzero length is a finite binary32")
    })
}

/// The state-AI advance ParticleClass's constructor leaves
/// (`0x0062B956..0x0062BA51`). A type without `Normalized=` (`+0x30F`) keeps
/// `StateAIAdvance=`. A normalized one takes the fewer of the ticks its
/// particle needs to cover the source-target distance along x and along y.
/// Each axis step is `|ftol(velocity × speed)|` stored as binary32,
/// and an axis counts only with a step: the `1e-6` test passes any whole step
/// from 1. The x ticks `|dx| / step` stay on the x87 (9999.0 without an x
/// step); the y ticks are stored as binary32 and replace them unless the x
/// ticks are smaller. The advance is the low byte of
/// `ftol(ticks / (FinalDamageState + 1) + 1.0)`, the state sign-extended; a
/// state of -1 divides by zero, which the masked x87 converts to 0. `speed`
/// is the particle's stored binary32 scalar (`+0xE4`).
pub(super) fn spawn_state_ai_advance(
    pt: &crate::rules::particle_type::ParticleType,
    source: IVec3,
    target: IVec3,
    velocity: [NativeF32Bits; 3],
    speed: NativeF32Bits,
) -> u8 {
    // `0x007EF920`.
    const NO_STEP_TICKS: NativeF32Bits = NativeF32Bits::from_bits(0x461C_3C00);
    if !pt.normalized {
        return pt.state_ai_advance;
    }
    let load = |bits| X87Chop53::load_f32(bits).expect("a particle's binary32 is finite");
    let speed = load(speed);
    let step = |component| X87Chop53::ftol_i32_low_masked(X87Chop53::mul(load(component), speed));
    let [step_x, step_y] = [
        step(velocity[0]).wrapping_abs(),
        step(velocity[1]).wrapping_abs(),
    ];
    let ticks = |from: i32, to: i32, step: i32| {
        let step = X87Chop53::store_f32(X87Chop53::load_i32(step))
            .expect("a whole step is a finite binary32");
        X87Chop53::div(
            X87Chop53::load_i32(from.wrapping_sub(to).wrapping_abs()),
            load(step),
        )
        .expect("an axis counts only with a step")
    };

    let mut best = if step_x > 0 {
        ticks(source.x, target.x, step_x)
    } else {
        load(NO_STEP_TICKS)
    };
    if step_y > 0 {
        let y_ticks = X87Chop53::store_f32(ticks(source.y, target.y, step_y))
            .expect("whole-lepton ticks are a finite binary32");
        if X87Chop53::compare(best, load(y_ticks)) != X87Ordering::Less {
            best = load(y_ticks);
        }
    }
    let divisor = X87Chop53::load_i32(i32::from(pt.final_damage_state as i8) + 1);
    let one = X87Chop53::load_f64(NativeF64Bits::ONE).expect("1.0 is finite");
    let advance = X87Chop53::div(best, divisor).map_or(0, |per_state| {
        X87Chop53::ftol_i32_low_masked(X87Chop53::add(per_state, one))
    });
    advance as u8
}

/// Fire-only variant: spawn one particle, then random-shuffle it into the last
/// `insert_range` slots so the stream looks varied. Returns `false` if the
/// underlying `spawn_particle` failed (cap reached or no `HoldsWhat`).
pub(super) fn spawn_particle_with_insert(
    sys: &mut ParticleSystem,
    coords: IVec3,
    insert_range: usize,
    rules: &RuleSet,
    sim: &mut Simulation,
) -> bool {
    if insert_range == 0 || !spawn_particle(sys, coords, rules, sim) {
        return false;
    }
    let count = sys.particles.len();
    if count < 2 {
        return true;
    }
    let actual_range = insert_range.min(count);
    let random_offset = sim.particle_rng().next_raw_abs_modulo(actual_range as u32) as usize;
    let insert_pos = count.saturating_sub(2).saturating_sub(random_offset);
    if insert_pos + 1 >= count {
        return true;
    }
    let p = sys.particles.pop().unwrap();
    sys.particles.insert(insert_pos + 1, p);
    true
}

#[cfg(test)]
mod tests {
    use crate::sim::rng::SimRng;
    #[test]
    fn original_96_receiver_height_rows_gate_constructor_and_rng() {
        use crate::map::resolved_terrain::{ResolvedTerrainGrid, test_flat_cell};
        use crate::sim::game_entity::GameEntity;
        let rows: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/receiver_smoke_height.json",
        ))
        .unwrap();
        assert_eq!(rows.as_array().unwrap().len(), 96);
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[VehicleTypes]\n0=TEST\n[TEST]\nStrength=100\nDamageParticleSystems=Sys,SysTwo\nDamageSmokeOffset=0,0,0\nDamSmkOffScrnRel=no\n[Particles]\n0=Smk\n[ParticleSystems]\n0=Sys\n1=SysTwo\n[Smk]\nBehavesLike=Smoke\nMaxEC=10\n[Sys]\nBehavesLike=Smoke\nHoldsWhat=Smk\n[SysTwo]\nBehavesLike=Smoke\nHoldsWhat=Smk\n")).unwrap();
        for row in rows.as_array().unwrap() {
            let input = &row["input"];
            let x = input["xy"][0].as_i64().unwrap() as i32;
            let y = input["xy"][1].as_i64().unwrap() as i32;
            let z = input["z"].as_i64().unwrap() as i32;
            let mut sim = Simulation::new();
            let id = sim.allocate_stable_id();
            let mut entity =
                GameEntity::test_default(id, "TEST", "A", (x >> 8) as u16, (y >> 8) as u16);
            entity.set_type_ref_for_test(sim.interner.intern("TEST"));
            entity.set_owner_for_test(sim.interner.intern("A"));
            entity.health.set_for_test(40);
            entity
                .position
                .set_sub_x_for_test(SimFixed::from_num(x & 255));
            entity
                .position
                .set_sub_y_for_test(SimFixed::from_num(y & 255));
            entity.position.set_exact_z_leptons_for_test(Some(z));
            entity.set_on_bridge(input["on_bridge"].as_bool().unwrap());
            let mut cell = test_flat_cell(entity.position.rx(), entity.position.ry());
            cell.level = input["level"].as_u64().unwrap() as u8;
            cell.slope_type = input["slope"].as_u64().unwrap() as u8;
            sim.resolved_terrain = Some(ResolvedTerrainGrid::from_cells(8, 8, vec![cell]));
            assert_eq!(
                sim.damage_particle_owner_height(&entity),
                row["output"]["height"].as_i64().unwrap() as i32
            );
            sim.substrate.entities.insert(entity);
            let before_rng = sim.scenario_rng.logical_state();
            sim.maintain_damage_smoke_after_receive(
                id,
                crate::sim::combat::damage::DamageState::Yellow,
                &rules,
            );
            let smoke = sim
                .entities()
                .get(id)
                .unwrap()
                .damage_smoke_system_id
                .system();
            assert_eq!(
                smoke.is_some(),
                row["output"]["spawn_admitted"] == true,
                "{row}"
            );
            if let Some(smoke) = smoke {
                assert_eq!(
                    sim.particle_systems().get(smoke).unwrap().coords,
                    IVec3::new(x, y, z)
                );
                assert_ne!(sim.scenario_rng.logical_state(), before_rng);
            } else {
                assert_eq!(sim.scenario_rng.logical_state(), before_rng);
                assert_eq!(sim.allocate_stable_id(), id + 1);
            }
        }
    }
    #[test]
    fn original_health_ratio_corpus_marks_owned_damage_smoke() {
        use crate::map::entities::EntityCategory;
        use crate::sim::combat::Health;
        use crate::sim::game_entity::GameEntity;
        for row in crate::sim::health_ratio_fixture::rows() {
            let mut rules = RuleSet::from_ini(&IniFile::from_str(&format!(
                "[VehicleTypes]\n0=TEST\n[TEST]\nStrength={}\n[Particles]\n0=Smk\n[ParticleSystems]\n0=Sys\n[Smk]\nBehavesLike=Smoke\nMaxEC=10\nMaxDC=4\nStartStateAI=0\nEndStateAI=10\nStateAIAdvance=4\n[Sys]\nBehavesLike=Smoke\nHoldsWhat=Smk\nParticleCap=10\nSpawnFrames=1\nLifetime=200\n",
                row.input.strength
            ))).unwrap();
            rules.general.condition_yellow = row.input.yellow();
            let mut sim = Simulation::new();
            let owner = sim.interner.intern("A");
            let type_ref = sim.interner.intern("TEST");
            let id = sim.allocate_stable_id();
            let entity = GameEntity::new_at_frame_zero_for_test(
                id,
                0,
                0,
                0,
                0,
                owner,
                Health::new(row.input.current),
                type_ref,
                EntityCategory::Unit,
                0,
                5,
                true,
            );
            sim.substrate.entities.insert(entity);
            let system_id = sim
                .spawn_particle_system(
                    ParticleSystemTypeId(0),
                    IVec3::ZERO,
                    Some(crate::sim::combat::TargetKind::Entity(id)),
                    Some(id),
                    IVec3::ZERO,
                    None,
                    &rules,
                )
                .unwrap();
            sim.substrate
                .entities
                .get_mut(id)
                .unwrap()
                .damage_smoke_system_id
                .set_for_test(Some(system_id));
            let before_rng = sim.scenario_rng.logical_state();
            sim.maintain_damage_smoke_after_receive(
                id,
                crate::sim::combat::damage::DamageState::Yellow,
                &rules,
            );
            assert_eq!(
                sim.particle_systems().get(system_id).unwrap().done_spawning,
                row.output.smoke_above_yellow,
                "{row:?}"
            );
            assert_eq!(
                sim.substrate
                    .entities
                    .get(id)
                    .unwrap()
                    .damage_smoke_system_id
                    .system(),
                Some(system_id)
            );
            assert_eq!(sim.particle_systems().len(), 1);
            assert_eq!(sim.scenario_rng.logical_state(), before_rng);
        }
    }
    use super::*;
    use crate::rules::ini_parser::IniFile;

    /// ParticleClass's constructor executed on each row
    /// (`tools/spatial_oracle/sqrt_approx_distances.py`), against
    /// `spawn_particle` for a `Normalized=` Fire type of the row's speed and
    /// `FinalDamageState=`: the binary32 vector bit for bit, the direction
    /// that vector toward zero within one `SimFixed` step, and the state-AI
    /// advance byte.
    #[test]
    fn normalized_spawn_matches_original_rows() {
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/sqrt_approx_distances.json",
        ))
        .unwrap();
        let rows = corpus["particle"].as_array().unwrap();
        for row in rows {
            let name = &row["name"];
            let [source, target] = [&row["source"], &row["target"]]
                .map(|v| IVec3::from_array(serde_json::from_value(v.clone()).unwrap()));
            let ini_text = format!(
                "[Particles]\n\
                 1=FireStream\n\
                 [FireStream]\n\
                 BehavesLike=Fire\n\
                 MaxEC=10\n\
                 Velocity={}\n\
                 StateAIAdvance=6\n\
                 FinalDamageState={}\n\
                 Normalized=yes\n\
                 [ParticleSystems]\n\
                 1=FireSys\n\
                 [FireSys]\n\
                 BehavesLike=Fire\n\
                 HoldsWhat=FireStream\n\
                 ParticleCap=5\n",
                row["speed"], row["final_damage_state"],
            );
            let rules = RuleSet::from_ini(&IniFile::from_str(&ini_text)).expect("rules parse");
            let mut sim = Simulation::new();
            let sys_id = sim
                .spawn_particle_system(
                    ParticleSystemTypeId(0),
                    source,
                    None,
                    None,
                    target,
                    None,
                    &rules,
                )
                .unwrap();
            let mut sys = sim.particle_systems_mut().take_for_tick(sys_id).unwrap();
            assert!(spawn_particle(&mut sys, source, &rules, &mut sim), "{name}");

            let particle = &sys.particles[0];
            let velocity: [u32; 3] = serde_json::from_value(row["velocity"].clone()).unwrap();
            assert_eq!(
                native_velocity(source, target).map(NativeF32Bits::bits),
                velocity,
                "{name}"
            );
            for (kept, bits) in particle.direction.into_iter().zip(velocity) {
                let native = f64::from(f32::from_bits(bits));
                let kept = kept.to_num::<f64>();
                assert!(
                    kept * native >= 0.0
                        && kept.abs() <= native.abs()
                        && native.abs() - kept.abs() < 1.0 / 65536.0,
                    "{name}: {kept} for {native}"
                );
            }
            assert_eq!(
                u64::from(particle.state_ai_advance),
                row["advance"].as_u64().unwrap(),
                "{name}"
            );
        }
        assert_eq!(rows.len(), 13);
    }

    /// Build a tiny RuleSet with one ParticleType + one ParticleSystemType.
    /// `behaves_like` selects which BehavesLike to assign on the system.
    /// `particle_cap` lets each test pin its own cap independently of the default.
    fn build_rules(behaves_like: &str, particle_cap: u32) -> RuleSet {
        let ini_text = format!(
            "[Particles]\n\
             1=Smk\n\
             [ParticleSystems]\n\
             1=Sys\n\
             [Smk]\n\
             BehavesLike=Smoke\n\
             MaxEC=10\n\
             MaxDC=4\n\
             StartStateAI=0\n\
             EndStateAI=10\n\
             StateAIAdvance=4\n\
             Translucency=0\n\
             [Sys]\n\
             BehavesLike={behaves_like}\n\
             HoldsWhat=Smk\n\
             ParticleCap={particle_cap}\n\
             SpawnFrames=1\n\
             Lifetime=200\n",
        );
        let ini = IniFile::from_str(&ini_text);
        RuleSet::from_ini(&ini).expect("rules parse")
    }

    #[test]
    fn spawn_returns_none_for_railgun_at_tier_2() {
        let rules = build_rules("Railgun", 50);
        let mut sim = Simulation::new();
        let result = sim.spawn_particle_system(
            ParticleSystemTypeId(0),
            IVec3::ZERO,
            None,
            None,
            IVec3::ZERO,
            None,
            &rules,
        );
        assert!(result.is_none());
    }

    #[test]
    fn spawn_returns_some_for_smoke() {
        let rules = build_rules("Smoke", 50);
        let mut sim = Simulation::new();
        let prior_object_id = sim.allocate_stable_id();
        let id = sim
            .spawn_particle_system(
                ParticleSystemTypeId(0),
                IVec3::new(100, 100, 0),
                None,
                None,
                IVec3::ZERO,
                None,
                &rules,
            )
            .expect("smoke system spawns");
        assert_eq!(id, prior_object_id + 1);
        assert_eq!(sim.particle_systems().len(), 1);
        assert_eq!(sim.live_object_order_snapshot(), vec![id]);
        let sys = sim.particle_systems().get(id).unwrap();
        assert_eq!(sys.coords, IVec3::new(100, 100, 0));
        assert_eq!(sys.lifetime, 200);
        assert_eq!(sys.facing, 0x1D);
    }

    #[test]
    fn spawn_particle_respects_particle_cap() {
        let rules = build_rules("Smoke", 3);
        let mut sim = Simulation::new();
        let sys_id = sim
            .spawn_particle_system(
                ParticleSystemTypeId(0),
                IVec3::ZERO,
                None,
                None,
                IVec3::ZERO,
                None,
                &rules,
            )
            .unwrap();
        *sim.particle_rng() = SimRng::new(1);
        let mut sys = sim.particle_systems_mut().take_for_tick(sys_id).unwrap();
        for _ in 0..10 {
            spawn_particle(&mut sys, IVec3::ZERO, &rules, &mut sim);
        }
        assert_eq!(sys.particles.len(), 3);
    }

    #[test]
    fn lifetime_extra_uses_raw_abs_modulo_single_draw() {
        // Use Gas to isolate the common lifetime roll; Smoke's preceding
        // speed draw is covered by the original constructor corpus.
        // MaxEC=10. seed=1 first raw draw is 0x78B76ED5 (+2_025_287_381);
        // abs(% 10) = 1 -> lifetime_remaining = 10 + 1 = 11. The raw-modulo helper
        // consumes EXACTLY ONE draw (no rejection loop); this pins both the value
        // and the single-advance contract.
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[Particles]\n0=Gas\n[Gas]\nBehavesLike=Gas\nMaxEC=10\n\
             [ParticleSystems]\n0=Sys\n[Sys]\nBehavesLike=Gas\n\
             HoldsWhat=Gas\nParticleCap=50\n",
        ))
        .unwrap();
        let mut sim = Simulation::new();
        let sys_id = sim
            .spawn_particle_system(
                ParticleSystemTypeId(0),
                IVec3::ZERO,
                None,
                None,
                IVec3::ZERO,
                None,
                &rules,
            )
            .unwrap();
        *sim.particle_rng() = SimRng::new(1);
        let mut sys = sim.particle_systems_mut().take_for_tick(sys_id).unwrap();
        spawn_particle(&mut sys, IVec3::ZERO, &rules, &mut sim);
        assert_eq!(sys.particles[0].lifetime_remaining, 11);

        // Exactly one raw draw consumed by the lifetime roll.
        let mut reference = SimRng::new(1);
        reference.next_u32();
        assert_eq!(sim.particle_rng().state(), reference.state());
    }

    #[test]
    fn spawn_particle_returns_false_when_holds_what_unset() {
        // [Sys] without HoldsWhat — minimal INI to leave holds_what = None.
        let ini_text = "[ParticleSystems]\n\
                        1=NoHold\n\
                        [NoHold]\n\
                        BehavesLike=Smoke\n\
                        ParticleCap=10\n";
        let ini = IniFile::from_str(ini_text);
        let rules = RuleSet::from_ini(&ini).expect("rules parse");
        let mut sim = Simulation::new();
        let sys_id = sim
            .spawn_particle_system(
                ParticleSystemTypeId(0),
                IVec3::ZERO,
                None,
                None,
                IVec3::ZERO,
                None,
                &rules,
            )
            .unwrap();
        *sim.particle_rng() = SimRng::new(1);
        let mut sys = sim.particle_systems_mut().take_for_tick(sys_id).unwrap();
        assert!(!spawn_particle(&mut sys, IVec3::ZERO, &rules, &mut sim));
        assert!(sys.particles.is_empty());
    }
}
