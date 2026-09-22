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
//! Tier 3 system types (`Spark`, `Railgun`) are accepted by the public entry
//! point but logged + skipped — runtime spawn returns `None`.

use super::{Particle, ParticleSystem};
use crate::rules::particle_system_type::{ParticleSystemBehavesLike, ParticleSystemTypeId};
use crate::rules::particle_type::ParticleBehavesLike;
use crate::rules::ruleset::RuleSet;
use crate::sim::intern::InternedId;
use crate::sim::rng::SimRng;
use crate::sim::world::Simulation;
use crate::util::fixed_math::{SIM_ONE, SIM_ZERO, SimFixed};
use crate::util::native_x87::{X87Chop53, sqrt_approx_f32};
use fixed::types::I48F16;
use glam::IVec3;

impl Simulation {
    /// Mark the attached Techno+310 smoke system for retirement while retaining
    /// its owner pointer. Native ParticleSystem virtual F8 (6301E0) only sets
    /// its done-spawning byte; physical deletion owns pointer expiry.
    pub(crate) fn retire_attached_damage_smoke(&mut self, stable_id: u64) {
        let system_id = self
            .substrate
            .entities
            .get(stable_id)
            .and_then(|entity| entity.damage_smoke_system_id);
        if let Some(system_id) = system_id
            && let Some(system) = self.particle_systems_mut().get_mut(system_id)
        {
            system.done_spawning = true;
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
                self.damage_smoke_owner_height(entity) < -10
            });
        if retire {
            self.retire_attached_damage_smoke(stable_id);
        }
    }
    /// Shared original Object5F5F40 adapter for the two distinct smoke gates.
    fn damage_smoke_owner_height(&self, entity: &crate::sim::game_entity::GameEntity) -> i32 {
        let raw = crate::sim::movement::ground_pose::position_world_coord(&entity.position);
        let surface = crate::sim::movement::ground_pose::ground_surface_z_at(
            [raw.x, raw.y],
            entity.on_bridge,
            self.resolved_terrain.as_ref(),
            None,
        )
        .unwrap_or(0);
        raw.z.wrapping_sub(surface)
    }

    /// Spawn a new particle system. Returns the new system's stable id, or
    /// `None` for a `Railgun` type, which is still unimplemented.
    ///
    /// gamemd-derived: `ParticleSystemClass::Constructor @ 0x0062DC50`. Spark
    /// systems are admitted and simulated — `particles/spark_spawn.rs` owns
    /// `ParticleSystemClass::AI_Spark @ 0x0062E840`'s burst half and
    /// `particles/spark.rs` the per-particle kernel. Railgun is still refused.
    ///
    /// The electric-bolt producer is wired: `sim/world/mod.rs`'s post-combat
    /// walk creates one `[CombatDamage] DefaultSparkSystem` per
    /// `IsElectricBolt=yes` discharge at the bolt's target endpoint, matching
    /// `EBolt::Init @ 0x004C2A60`'s construction at `0x004C2B30` — no owner
    /// house, no attachment, handle discarded, no RNG consumed. That is the
    /// most frequent of native's five Spark producers by a wide margin: every
    /// Tesla Coil, Tesla Trooper, Tesla Tank, `AssaultBolt`, `EiffelBolt`,
    /// `CRElectricBolt` shot. It does NOT cover shrapnel bolts, which reach
    /// `CreateElectricBolt` through a different seam — see the residual below.
    ///
    /// RESIDUAL (GSI-05.13) — the shrapnel seam and four other native
    /// producers are not wired. Their reachability is settled rather than
    /// assumed:
    /// - `BulletClass::SpawnShrapnel @ 0x0046A310` calls `CreateElectricBolt`
    ///   once per shrapnel bolt when the CHILD weapon carries `IsElectricBolt`.
    ///   REACHABLE: `[TankBoltE]` and `[ElectricBoltE]` — the elite Tesla Tank
    ///   and elite Tesla Coil weapons — use `Projectile=Electricbounce`, whose
    ///   `ShrapnelWeapon=TeslaFragment, ShrapnelCount=2`, and
    ///   `[TeslaFragment] IsElectricBolt=true`. So an elite Tesla shot throws
    ///   two more spark systems this engine does not create.
    ///   (`[ElectricFragment]` also sets the flag but is named by no stock
    ///   `ShrapnelWeapon=`, so it is dead and deliberately not listed as a
    ///   trigger.) The producer wired above walks `fire_events`, while
    ///   `emit_projectile_shrapnel` pushes `projectile_spawns`, so wiring this
    ///   arm means a second call site rather than a wider filter.
    /// - `TechnoClass::Fire_At @ 0x006FF1EC`, on `WeaponType+0x12A`
    ///   (`UseSparkParticles`), spawning `WeaponType+0x11C`
    ///   (`AttachedParticleSystem`) into `Techno+0x308`, one at a time.
    ///   REACHABLE: stock authors it once, on `[RepairBullet]`
    ///   (`AttachedParticleSystem=WeldingSys`), carried by the IFV's
    ///   `Weapon2`/`EliteWeapon2` — the repair IFV's welding sparks.
    /// - `CaptureManagerClass::Update @ 0x00471C15`, mind-control overload past
    ///   the first `OverloadCount` tier. REACHABLE in any Yuri match; five
    ///   iterations, each taking two `RandomRanged(-200, 200)` draws first.
    /// - `WarpAttachClass::UpdateAttack @ 0x0062A103`, Chrono Legionnaire
    ///   erasure. REACHABLE.
    /// - `UnitClass::AI @ 0x007361A4`, mid-deploy, gated on `Techno+0x1C8` and
    ///   a coordinate/frame modulo. REACHABLE but rare — a few frames per MCV
    ///   or Slave Miner deploy, taking two `RandomRanged(-100, 100)` draws.
    ///
    /// - Trigger: an elite Tesla shot, repairing with an IFV, overloading a
    ///   mind-controller, erasing with a Chrono Legionnaire, or deploying an
    ///   MCV.
    /// - Player effect: those four throw no sparks. The IFV repair arm is the
    ///   one a player watches — a repair beam with no welding shower.
    /// - Frequency: the shrapnel arm is the common one — it follows every
    ///   elite Tesla Tank and elite Tesla Coil discharge, and veterancy makes
    ///   those ordinary in a long Soviet game. The IFV arm follows every repair
    ///   tick; the deploy arm is a handful of frames a match.
    /// - Downstream risk: the `Fire_At` arm needs the `Techno+0x308` slot and
    ///   its clearing writer, which is UNCHECKED — the offset is shared across
    ///   several classes and the search for the writer was not settled. The
    ///   overload and deploy arms take RNG draws *before* their spawn, so both
    ///   move the shared stream and want their own slice with a re-baseline.
    ///
    /// NOT_APPLICABLE_PROVEN, and recorded so it is not re-attempted: the
    /// `DamageParticleSystems=` Spark producer in
    /// `TechnoClass::AI_Update @ 0x006FADB3` is Tiberian Sun legacy and never
    /// runs in stock YR, despite 126 of the 141 stock `DamageParticleSystems=`
    /// entries naming a Spark system. Its top gate at `0x006FACD9` reads
    /// `TechnoType+0xC8F`, whose only setter is `0x005243E7` in
    /// `InfantryTypeClass::ReadINI`, conditional on the section's `Cyborg=`
    /// bool — and no stock INI authors `Cyborg=` (`rulesmd.ini:3581` mentions
    /// it only in a TS-inherited comment). Implementing it would both add
    /// sparks retail does not show and consume `ScenarioClass::Random` draws
    /// gamemd does not take, which desyncs. The `Smoke` producer below is a
    /// different function (`TechnoClass::ReceiveDamage @ 0x00701900`) and is
    /// not affected.
    #[allow(clippy::too_many_arguments)]
    pub fn spawn_particle_system(
        &mut self,
        type_id: ParticleSystemTypeId,
        coords: IVec3,
        attached_entity: Option<u64>,
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
        let directionless = pst.spawn_direction == IVec3::ZERO;
        let stable_id = self.allocate_stable_id();
        let sys = ParticleSystem {
            stable_id,
            in_logic_vector: false,
            type_id,
            coords,
            offset: IVec3::ZERO,
            particles: Vec::new(),
            spawn_timer: SimFixed::from_num(pst.spawn_frames as i32),
            lifetime: pst.lifetime,
            spark_spawn_frames: pst.spark_spawn_frames as i32,
            facing: 0x1D,
            directionless,
            attached_entity,
            owner_entity,
            target_coords,
            owner_house,
            done_spawning: false,
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
        let current_system = entity.damage_smoke_system_id;

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
                let object = rules.object(self.interner.resolve(entity.type_ref()))?;
                let offset = damage_smoke_offset(object);
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
            .is_none_or(|entity| self.damage_smoke_owner_height(entity) <= -10)
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
            entity.damage_smoke_system_id = Some(system_id);
        }
    }
}

/// TechnoTypeClass::GetDamageParticleOffset @0x007178C0. The screen-relative
/// arm first runs the retail isometric-pixel matrix, then folds authored Z
/// into all three world axes through Sqrt_Approx/ftol.
fn damage_smoke_offset(object: &crate::rules::object_type::ObjectType) -> IVec3 {
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
    spawn_origin: IVec3,
    rules: &RuleSet,
    rng: &mut SimRng,
) -> bool {
    let pst = rules.particle_system_type(sys.type_id);
    let Some(pt_id) = pst.holds_what else {
        return false;
    };
    if sys.particles.len() >= pst.particle_cap as usize {
        return false;
    }
    let pt = rules.particle_type(pt_id);
    let direction = normalized_direction(coords, sys.target_coords);
    let state_ai_advance = spawn_state_ai_advance(pt, coords, sys.target_coords, direction);

    let lifetime_extra = if pt.behaves_like == ParticleBehavesLike::Railgun {
        rng.next_raw_abs_modulo(10) as i16
    } else {
        let base = (pt.max_ec as u32).max(1);
        rng.next_raw_abs_modulo(base) as i16
    };
    let lifetime_remaining = (pt.max_ec as i16).saturating_add(lifetime_extra);

    sys.particles.push(Particle {
        type_id: pt_id,
        coords,
        previous_coords: spawn_origin,
        origin: coords,
        direction,
        velocity: pt.velocity,
        lifetime_remaining,
        damage_counter: pt.max_dc as i16,
        state_ai_advance,
        animation_state: pt.start_state_ai,
        translucency: pt.translucency,
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
        state_advance_counter: 0,
    });
    true
}

fn normalized_direction(source: IVec3, target: IVec3) -> [SimFixed; 3] {
    let dx = target.x - source.x;
    let dy = target.y - source.y;
    let dz = target.z - source.z;
    if dx == 0 && dy == 0 && dz == 0 {
        return [SIM_ZERO; 3];
    }

    let dx_w = I48F16::from_num(dx);
    let dy_w = I48F16::from_num(dy);
    let dz_w = I48F16::from_num(dz);
    let dist = sqrt_i48(dx_w * dx_w + dy_w * dy_w + dz_w * dz_w);
    if dist <= I48F16::ZERO {
        return [SIM_ZERO; 3];
    }

    [
        i48_to_sim(dx_w / dist),
        i48_to_sim(dy_w / dist),
        i48_to_sim(dz_w / dist),
    ]
}

fn spawn_state_ai_advance(
    pt: &crate::rules::particle_type::ParticleType,
    source: IVec3,
    target: IVec3,
    direction: [SimFixed; 3],
) -> u8 {
    if !pt.normalized {
        return pt.state_ai_advance;
    }

    let step_x = ftol_chop(direction[0] * pt.velocity).abs();
    let step_y = ftol_chop(direction[1] * pt.velocity).abs();
    let mut best_ticks = SimFixed::from_num(9999);

    if step_x > 0 {
        best_ticks = SimFixed::from_num((source.x - target.x).abs()) / SimFixed::from_num(step_x);
    }
    if step_y > 0 {
        let y_ticks = SimFixed::from_num((source.y - target.y).abs()) / SimFixed::from_num(step_y);
        if best_ticks >= y_ticks {
            best_ticks = y_ticks;
        }
    }

    let divisor = SimFixed::from_num(u16::from(pt.final_damage_state) + 1);
    let advance: i32 = ftol_chop(best_ticks / divisor + SIM_ONE);
    advance as u8
}

fn ftol_chop(val: SimFixed) -> i32 {
    let bits = i64::from(val.to_bits());
    if bits >= 0 {
        (bits >> 16) as i32
    } else {
        -((-bits) >> 16) as i32
    }
}

fn sqrt_i48(val: I48F16) -> I48F16 {
    if val <= I48F16::ZERO {
        return I48F16::ZERO;
    }
    let two = I48F16::from_num(2);
    let mut guess = if val < two { val } else { val / two };
    for _ in 0..16 {
        if guess <= I48F16::ZERO {
            return I48F16::ZERO;
        }
        guess = (guess + val / guess) / two;
    }
    guess
}

fn i48_to_sim(val: I48F16) -> SimFixed {
    let bits = val.to_bits().clamp(i32::MIN as i64, i32::MAX as i64) as i32;
    SimFixed::from_bits(bits)
}

/// Fire-only variant: spawn one particle, then random-shuffle it into the last
/// `insert_range` slots so the stream looks varied. Returns `false` if the
/// underlying `spawn_particle` failed (cap reached or no `HoldsWhat`).
pub(super) fn spawn_particle_with_insert(
    sys: &mut ParticleSystem,
    coords: IVec3,
    spawn_origin: IVec3,
    insert_range: usize,
    rules: &RuleSet,
    rng: &mut SimRng,
) -> bool {
    if insert_range == 0 || !spawn_particle(sys, coords, spawn_origin, rules, rng) {
        return false;
    }
    let count = sys.particles.len();
    if count < 2 {
        return true;
    }
    let actual_range = insert_range.min(count);
    let random_offset = rng.next_raw_abs_modulo(actual_range as u32) as usize;
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
    #[test]
    fn original_96_receiver_height_rows_gate_constructor_and_rng() {
        use crate::map::resolved_terrain::{ResolvedTerrainGrid, test_flat_cell};
        use crate::sim::game_entity::GameEntity;
        let rows: serde_json::Value = serde_json::from_str(include_str!(
            "../../../tools/spatial_oracle/receiver_smoke_height.json"
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
            entity.type_ref = sim.interner.intern("TEST");
            entity.owner = sim.interner.intern("A");
            entity.health.current = 40;
            entity.position.sub_x = SimFixed::from_num(x & 255);
            entity.position.sub_y = SimFixed::from_num(y & 255);
            entity.position.exact_z_leptons = Some(z);
            entity.on_bridge = input["on_bridge"].as_bool().unwrap();
            let mut cell = test_flat_cell(entity.position.rx, entity.position.ry);
            cell.level = input["level"].as_u64().unwrap() as u8;
            cell.slope_type = input["slope"].as_u64().unwrap() as u8;
            sim.resolved_terrain = Some(ResolvedTerrainGrid::from_cells(8, 8, vec![cell]));
            assert_eq!(
                sim.damage_smoke_owner_height(&entity),
                row["output"]["height"].as_i64().unwrap() as i32
            );
            sim.substrate.entities.insert(entity);
            let before_rng = sim.scenario_rng.logical_state();
            sim.maintain_damage_smoke_after_receive(
                id,
                crate::sim::combat::damage::DamageState::Yellow,
                &rules,
            );
            let smoke = sim.entities().get(id).unwrap().damage_smoke_system_id;
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
        use crate::sim::components::Health;
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
                Health {
                    current: row.input.current,
                },
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
                    Some(id),
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
                .damage_smoke_system_id = Some(system_id);
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
                    .damage_smoke_system_id,
                Some(system_id)
            );
            assert_eq!(sim.particle_systems().len(), 1);
            assert_eq!(sim.scenario_rng.logical_state(), before_rng);
        }
    }
    use super::*;
    use crate::rules::ini_parser::IniFile;

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
    fn gsi_05_13_spawn_admits_spark_systems() {
        // `ParticleSystemClass::AI_Spark @ 0x0062E840` is implemented, so the
        // Tier-3 refusal that used to sit here is gone for Spark. Railgun is
        // still refused; see the test below.
        let rules = build_rules("Spark", 50);
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
        let id = result.expect("Spark systems are admitted");
        assert_eq!(sim.particle_systems().len(), 1);
        let system = sim.particle_systems().get(id).expect("stored system");
        assert_eq!(system.facing, 0x1D, "ParticleSystem+0xF4 starts at 0x1D");
        assert!(!system.done_spawning);
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
        assert!(sys.directionless);
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
        let mut rng = SimRng::new(1);
        let sys = sim.particle_systems_mut().get_mut(sys_id).unwrap();
        for _ in 0..10 {
            spawn_particle(sys, IVec3::ZERO, IVec3::ZERO, &rules, &mut rng);
        }
        assert_eq!(sys.particles.len(), 3);
    }

    #[test]
    fn lifetime_extra_uses_raw_abs_modulo_single_draw() {
        // MaxEC=10 (Smk). seed=1 first raw draw is 0x78B76ED5 (+2_025_287_381);
        // abs(% 10) = 1 -> lifetime_remaining = 10 + 1 = 11. The raw-modulo helper
        // consumes EXACTLY ONE draw (no rejection loop); this pins both the value
        // and the single-advance contract.
        let rules = build_rules("Smoke", 50);
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
        let mut rng = SimRng::new(1);
        let sys = sim.particle_systems_mut().get_mut(sys_id).unwrap();
        spawn_particle(sys, IVec3::ZERO, IVec3::ZERO, &rules, &mut rng);
        assert_eq!(sys.particles[0].lifetime_remaining, 11);

        // Exactly one raw draw consumed by the lifetime roll.
        let mut reference = SimRng::new(1);
        reference.next_u32();
        assert_eq!(rng.state(), reference.state());
    }

    #[test]
    fn spawn_with_insert_does_not_exceed_cap() {
        let rules = build_rules("Fire", 5);
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
        let mut rng = SimRng::new(1);
        let sys = sim.particle_systems_mut().get_mut(sys_id).unwrap();
        for _ in 0..10 {
            spawn_particle_with_insert(sys, IVec3::ZERO, IVec3::ZERO, 3, &rules, &mut rng);
        }
        assert_eq!(sys.particles.len(), 5);
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
        let mut rng = SimRng::new(1);
        let sys = sim.particle_systems_mut().get_mut(sys_id).unwrap();
        assert!(!spawn_particle(
            sys,
            IVec3::ZERO,
            IVec3::ZERO,
            &rules,
            &mut rng
        ));
        assert!(sys.particles.is_empty());
    }

    #[test]
    fn normalized_particle_rewrites_state_ai_advance_from_xy_travel_time() {
        let ini_text = "[Particles]\n\
                        1=FireStream\n\
                        [FireStream]\n\
                        BehavesLike=Fire\n\
                        MaxEC=10\n\
                        Velocity=28.0\n\
                        StateAIAdvance=6\n\
                        StartStateAI=1\n\
                        EndStateAI=19\n\
                        FinalDamageState=14\n\
                        Normalized=yes\n\
                        [ParticleSystems]\n\
                        1=FireSys\n\
                        [FireSys]\n\
                        BehavesLike=Fire\n\
                        HoldsWhat=FireStream\n\
                        ParticleCap=5\n";
        let ini = IniFile::from_str(ini_text);
        let rules = RuleSet::from_ini(&ini).expect("rules parse");
        let mut sim = Simulation::new();
        let sys_id = sim
            .spawn_particle_system(
                ParticleSystemTypeId(0),
                IVec3::new(0, 0, 0),
                None,
                None,
                IVec3::new(420, 0, 0),
                None,
                &rules,
            )
            .unwrap();
        let mut rng = SimRng::new(1);
        let sys = sim.particle_systems_mut().get_mut(sys_id).unwrap();

        assert!(spawn_particle(
            sys,
            IVec3::new(0, 0, 0),
            IVec3::new(0, 0, 0),
            &rules,
            &mut rng
        ));

        let particle = &sys.particles[0];
        assert_eq!(particle.direction, [SIM_ONE, SIM_ZERO, SIM_ZERO]);
        // step_x=trunc(1*28)=28; best_ticks=420/28=15;
        // advance=trunc(15/(FinalDamageState+1) + 1)=trunc(2)=2.
        assert_eq!(particle.state_ai_advance, 2);
    }

    #[test]
    fn normalized_particle_uses_3d_direction_but_only_xy_tick_candidates() {
        let ini_text = "[Particles]\n\
                        1=FireStream\n\
                        [FireStream]\n\
                        BehavesLike=Fire\n\
                        MaxEC=10\n\
                        Velocity=28.0\n\
                        StateAIAdvance=6\n\
                        FinalDamageState=14\n\
                        Normalized=yes\n\
                        [ParticleSystems]\n\
                        1=FireSys\n\
                        [FireSys]\n\
                        BehavesLike=Fire\n\
                        HoldsWhat=FireStream\n\
                        ParticleCap=5\n";
        let ini = IniFile::from_str(ini_text);
        let rules = RuleSet::from_ini(&ini).expect("rules parse");
        let mut sim = Simulation::new();
        let sys_id = sim
            .spawn_particle_system(
                ParticleSystemTypeId(0),
                IVec3::new(0, 0, 0),
                None,
                None,
                IVec3::new(300, 400, 1200),
                None,
                &rules,
            )
            .unwrap();
        let mut rng = SimRng::new(1);
        let sys = sim.particle_systems_mut().get_mut(sys_id).unwrap();

        assert!(spawn_particle(
            sys,
            IVec3::new(0, 0, 0),
            IVec3::ZERO,
            &rules,
            &mut rng
        ));

        let particle = &sys.particles[0];
        assert!(particle.direction[2] > SIM_ZERO);
        // Full 3D normalization makes x/y component steps small:
        // trunc(300/1300*28)=6, trunc(400/1300*28)=8.
        // X and Y candidates are both 50 ticks; Z is not considered.
        // advance=trunc(50/15 + 1)=4, replacing INI StateAIAdvance=6.
        assert_eq!(particle.state_ai_advance, 4);
    }

    #[test]
    fn normalized_particle_stores_low_byte_of_rewritten_advance() {
        let ini_text = "[Particles]\n\
                        1=FireStream\n\
                        [FireStream]\n\
                        BehavesLike=Fire\n\
                        MaxEC=10\n\
                        Velocity=1.0\n\
                        StateAIAdvance=6\n\
                        FinalDamageState=0\n\
                        Normalized=yes\n\
                        [ParticleSystems]\n\
                        1=FireSys\n\
                        [FireSys]\n\
                        BehavesLike=Fire\n\
                        HoldsWhat=FireStream\n\
                        ParticleCap=5\n";
        let ini = IniFile::from_str(ini_text);
        let rules = RuleSet::from_ini(&ini).expect("rules parse");
        let mut sim = Simulation::new();
        let sys_id = sim
            .spawn_particle_system(
                ParticleSystemTypeId(0),
                IVec3::ZERO,
                None,
                None,
                IVec3::new(300, 0, 0),
                None,
                &rules,
            )
            .unwrap();
        let mut rng = SimRng::new(1);
        let sys = sim.particle_systems_mut().get_mut(sys_id).unwrap();

        assert!(spawn_particle(
            sys,
            IVec3::ZERO,
            IVec3::ZERO,
            &rules,
            &mut rng
        ));

        // advance=trunc(300/1/(0+1)+1)=301; byte store keeps 45.
        assert_eq!(sys.particles[0].state_ai_advance, 45);
    }
}

#[cfg(test)]
mod gsi_05_13_electric_bolt_sparks {
    use crate::rules::ini_parser::IniFile;
    use crate::rules::ruleset::RuleSet;
    use crate::sim::command::{Command, CommandEnvelope};
    use crate::sim::components::Health;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::pathfinding::PathGrid;
    use crate::sim::world::{RevealOutcome, Simulation};
    use std::collections::BTreeMap;

    /// A Tesla-shaped fixture: one `IsElectricBolt=yes` weapon, the
    /// `[CombatDamage] DefaultSparkSystem` key it resolves through, and the
    /// stock-shaped Spark system and particle it names.
    fn tesla_rules() -> RuleSet {
        let ini = IniFile::from_str(
            "[VehicleTypes]\n0=TESLA\n1=TARGET\n\n\
             [TESLA]\nStrength=300\nArmor=heavy\nSpeed=6\nPrimary=CoilBolt\n\n\
             [TARGET]\nStrength=400\nArmor=heavy\nSpeed=6\n\n\
             [CoilBolt]\nDamage=40\nROF=50\nRange=6\nWarhead=AP\nIsElectricBolt=yes\n\n\
             [AP]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\n\
             [CombatDamage]\nDefaultSparkSystem=SparkSys\n\n\
             [ParticleSystems]\n1=SparkSys\n\n\
             [SparkSys]\nBehavesLike=Spark\nHoldsWhat=Spark\nParticleCap=6\n\
             SparkSpawnFrames=1\nSpawnSparkPercentage=1\nLightSize=15\nLifetime=200\n\n\
             [Particles]\n1=Spark\n\n\
             [Spark]\nBehavesLike=Spark\nMaxEC=500\nXVelocity=10\nYVelocity=10\n\
             MinZVelocity=40\nZVelocityRange=15\n\
             ColorList=(255,255,255),(200,200,80),(200,10,10),(0,0,0)\nColorSpeed=.13\n",
        );
        RuleSet::from_ini(&ini).expect("tesla rules parse")
    }

    fn unit(id: u64, type_ref: &str, rx: u16, ry: u16, owner: &str, hp: i32) -> GameEntity {
        let mut entity = GameEntity::test_default(id, type_ref, owner, rx, ry);
        entity.health = Health { current: hp };
        entity
    }

    #[test]
    fn firing_an_electric_bolt_weapon_spawns_a_spark_system_at_the_target() {
        // `EBolt::Init @ 0x004C2A60` constructs one particle system per bolt at
        // `0x004C2B30` from `Rules+0x1020` (`DefaultSparkSystem`) at the bolt's
        // target endpoint, and discards the handle. Before this row a Tesla
        // discharge produced nothing at all.
        let rules = tesla_rules();
        let mut sim = Simulation::new();
        sim.input_delay_ticks = 0;
        // Ids come from the shared allocator: a particle system takes one from
        // the same space, and the store asserts they never collide.
        let attacker = sim.allocate_stable_id();
        let target = sim.allocate_stable_id();
        sim.substrate
            .entities
            .insert(unit(attacker, "TESLA", 5, 5, "Americans", 300));
        sim.substrate
            .entities
            .insert(unit(target, "TARGET", 8, 5, "Soviet", 400));
        // The test interner is a thread-local the inserts above write into, so
        // it must be cloned into the sim AFTER they run.
        sim.interner = crate::sim::intern::test_interner();
        assert!(matches!(
            sim.reveal(attacker),
            RevealOutcome::Revealed { .. }
        ));
        assert!(matches!(sim.reveal(target), RevealOutcome::Revealed { .. }));

        let owner_id = sim.interner.intern("Americans");
        let grid = PathGrid::test_all_passable(64, 64);
        let height_map: BTreeMap<(u16, u16), u8> = BTreeMap::new();

        sim.queue_command(CommandEnvelope::new(
            owner_id,
            sim.session.tick + 1,
            Command::Attack {
                attacker_id: attacker,
                target_id: target,
            },
        ));

        let mut spark_system_id = None;
        for _ in 0..200 {
            let pending = sim.take_due_commands();
            sim.advance_tick(&pending, Some(&rules), &height_map, Some(&grid), None, 100);
            if let Some((&id, _)) = sim.particle_systems().iter().next() {
                spark_system_id = Some(id);
                break;
            }
        }

        let id = spark_system_id.expect("an electric-bolt discharge spawns a spark system");
        let system = sim.particle_systems().get(id).expect("stored system");
        assert_eq!(
            rules.particle_system_type(system.type_id).behaves_like,
            crate::rules::particle_system_type::ParticleSystemBehavesLike::Spark
        );
        // The bolt's target endpoint: the target sits at cell (8, 5), and
        // `GameEntity::test_default` centres it in the cell.
        assert_eq!(system.coords.x / 256, 8);
        assert_eq!(system.coords.y / 256, 5);
        assert!(
            system.owner_entity.is_none() && system.attached_entity.is_none(),
            "EBolt passes neither an owner house nor an attachment object"
        );
    }

    #[test]
    fn a_weapon_without_the_electric_bolt_flag_spawns_no_spark_system() {
        // The discriminator: the same fixture with `IsElectricBolt` absent must
        // produce nothing, or the test above would pass on any fire event.
        let ini_text = "[VehicleTypes]\n0=TESLA\n1=TARGET\n\n\
             [TESLA]\nStrength=300\nArmor=heavy\nSpeed=6\nPrimary=CoilBolt\n\n\
             [TARGET]\nStrength=400\nArmor=heavy\nSpeed=6\n\n\
             [CoilBolt]\nDamage=40\nROF=50\nRange=6\nWarhead=AP\n\n\
             [AP]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\n\
             [CombatDamage]\nDefaultSparkSystem=SparkSys\n\n\
             [ParticleSystems]\n1=SparkSys\n\n\
             [SparkSys]\nBehavesLike=Spark\nHoldsWhat=Spark\nParticleCap=6\n\
             SparkSpawnFrames=1\nSpawnSparkPercentage=1\nLifetime=200\n\n\
             [Particles]\n1=Spark\n\n\
             [Spark]\nBehavesLike=Spark\nMaxEC=500\nXVelocity=10\nYVelocity=10\n\
             MinZVelocity=40\nZVelocityRange=15\n";
        let rules = RuleSet::from_ini(&IniFile::from_str(ini_text)).expect("rules parse");
        let mut sim = Simulation::new();
        sim.input_delay_ticks = 0;
        // Ids come from the shared allocator: a particle system takes one from
        // the same space, and the store asserts they never collide.
        let attacker = sim.allocate_stable_id();
        let target = sim.allocate_stable_id();
        sim.substrate
            .entities
            .insert(unit(attacker, "TESLA", 5, 5, "Americans", 300));
        sim.substrate
            .entities
            .insert(unit(target, "TARGET", 8, 5, "Soviet", 400));
        // The test interner is a thread-local the inserts above write into, so
        // it must be cloned into the sim AFTER they run.
        sim.interner = crate::sim::intern::test_interner();
        assert!(matches!(
            sim.reveal(attacker),
            RevealOutcome::Revealed { .. }
        ));
        assert!(matches!(sim.reveal(target), RevealOutcome::Revealed { .. }));

        let owner_id = sim.interner.intern("Americans");
        let grid = PathGrid::test_all_passable(64, 64);
        let height_map: BTreeMap<(u16, u16), u8> = BTreeMap::new();
        sim.queue_command(CommandEnvelope::new(
            owner_id,
            sim.session.tick + 1,
            Command::Attack {
                attacker_id: attacker,
                target_id: target,
            },
        ));

        let mut fired = false;
        for _ in 0..200 {
            let pending = sim.take_due_commands();
            sim.advance_tick(&pending, Some(&rules), &height_map, Some(&grid), None, 100);
            if !sim.fire_events.is_empty() {
                fired = true;
            }
        }
        assert!(
            fired,
            "the fixture must actually fire, or it proves nothing"
        );
        assert_eq!(
            sim.particle_systems().len(),
            0,
            "only IsElectricBolt weapons spawn the spark system"
        );
    }
}
