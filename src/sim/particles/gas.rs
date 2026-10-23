//! Virus gas: the shared Scenario system, GasAI62BD50, Move62D2A0 and
//! reverse SystemAI62E6D0. Evidence: tools/spatial_oracle/virus_gas.
//!
//! The system stays in storage during damage: killing an animal can append a
//! particle synchronously. AI visits the original reverse list; the subsequent
//! Move/destruction walk includes those new births, as the original does.

use super::spark::{SparkMotionStep, resolve_collision};
use super::spark_world::{SparkCollisionWorld, constructor_ground_height};
use super::wind::{GAS_WIND_DX, GAS_WIND_DY};
use super::{Particle, ParticleSystem};
use crate::rules::particle_type::{ParticleBehavesLike, ParticleTypeId};
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::{
    EntityDamageEvent, RAD_NO_ATTACKER, ReceiverCallFlags, TerrainDamageEvent,
};
use crate::sim::intern::InternedId;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::occupancy::CellObjectMember;
use crate::sim::world::ObjectSubstrate;
use crate::sim::world::{Simulation, UninitContext};
use crate::util::native_x87::{MaskedX87Chop53 as X87, NativeF32Bits, adjust_for_z_standard};
use glam::IVec3;

impl Simulation {
    /// Setup684FF0..68503B constructs the literal GasCloudSys once after
    /// ClearScene. Constructor62DC50 and the ordinary store own its ID/Logic.
    pub(crate) fn initialize_gas_cloud_system(&mut self, rules: &RuleSet) -> Option<u64> {
        if let Some(id) = self.particle_systems().gas_cloud_system {
            return Some(id);
        }
        let kind = rules.ps_type_id_by_name("GasCloudSys")?;
        let id = self.spawn_particle_system(
            kind,
            IVec3::new(0xA80, 0xA80, 0),
            None,
            None,
            IVec3::ZERO,
            None,
            rules,
        )?;
        self.substrate.particle_systems.gas_cloud_system = Some(id);
        Some(id)
    }

    /// ParticleSystem62E430: construct the explicit ParticleType and append.
    /// +C4 is DynamicVector capacity, not ParticleSystemType.ParticleCap.
    pub(crate) fn spawn_gas_particle(
        &mut self,
        kind: ParticleTypeId,
        coords: IVec3,
        rules: &RuleSet,
    ) -> Option<u64> {
        let system = self.initialize_gas_cloud_system(rules)?;
        let particle = Particle::new(kind, coords, IVec3::ZERO, rules.particle_type(kind), self);
        let id = particle.stable_id();
        self.substrate
            .particle_systems
            .get_mut(system)?
            .particles
            .push(particle);
        Some(id)
    }
}

fn reverse_ids(system: &ParticleSystem) -> Vec<(usize, u64)> {
    system
        .particles
        .iter()
        .enumerate()
        .rev()
        .map(|(index, p)| (index, p.stable_id()))
        .collect()
}

fn particle(sim: &Simulation, system: u64, index: usize, id: u64) -> Option<Particle> {
    let particles = &sim.particle_systems().get(system)?.particles;
    // Ordinary callbacks only append, and reverse removals preserve lower
    // indices. Avoid scanning the entire shared cloud list for every visit;
    // re-resolve by identity only if a synchronous callback compacted it.
    particles
        .get(index)
        .filter(|p| p.stable_id() == id)
        .or_else(|| particles.iter().find(|p| p.stable_id() == id))
        .cloned()
}

fn commit_particle(substrate: &mut ObjectSubstrate, system: u64, index: usize, updated: Particle) {
    if let Some(sys) = substrate.particle_systems.get_mut(system) {
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

pub(super) fn tick_system(
    sim: &mut Simulation,
    rules: &RuleSet,
    system: u64,
    context: UninitContext<'_>,
) {
    let Some(sys) = sim.particle_systems().get(system) else {
        return;
    };
    let owner_house = sys.owner_house;
    let ids = reverse_ids(sys);
    for (index, id) in ids {
        let Some(mut p) = particle(sim, system, index, id) else {
            continue;
        };
        if rules.particle_type(p.type_id).behaves_like == ParticleBehavesLike::Gas {
            tick_particle(&mut p, sim, rules, owner_house, context);
            super::system_ai::finish_lifetime(&mut p);
        } else {
            super::system_ai::tick_particle(&mut p, sim, rules);
        }
        commit_particle(&mut sim.substrate, system, index, p);
    }
    let Some(sys) = sim.particle_systems().get(system) else {
        return;
    };
    for (index, id) in reverse_ids(sys) {
        let Some(mut p) = particle(sim, system, index, id) else {
            continue;
        };
        if p.marked_for_deletion {
            let pt = rules.particle_type(p.type_id);
            if let Some(next) = pt.next_particle {
                let coords = offset(p.coords, pt.next_particle_offset);
                let mut child =
                    Particle::new(next, coords, IVec3::ZERO, rules.particle_type(next), sim);
                //62E7EB..62E811 copies E4 and C0/C4/C8, not translucency
                //or CC/D0/D4. The constructor still takes its lifetime draw.
                child.velocity = p.velocity;
                [child.drift_x, child.drift_y, child.drift_z] = [p.drift_x, p.drift_y, p.drift_z];
                sim.substrate
                    .particle_systems
                    .get_mut(system)
                    .unwrap()
                    .particles
                    .push(child);
            }
            let parent = sim
                .substrate
                .particle_systems
                .get_mut(system)
                .and_then(|sys| sys.remove_expired_child(id));
            if let Some(parent) = parent {
                sim.retire_particle(parent, rules);
            }
        } else if rules.particle_type(p.type_id).behaves_like == ParticleBehavesLike::Gas {
            move_gas(&mut p, sim, rules);
            commit_particle(&mut sim.substrate, system, index, p);
        }
    }
}

/// Complete Gas body; the dispatcher separately owns its word-EC tail.
/// Shared Spark query/reflection owners implement the identical bridge,
/// ground/contact, building/wall and slope-matrix dependency.
pub(super) fn tick_particle(
    p: &mut Particle,
    sim: &mut Simulation,
    rules: &RuleSet,
    owner_house: Option<InternedId>,
    context: UninitContext<'_>,
) {
    if sim.session.binary_frame() & 1 == 0 {
        if sim.particle_rng().next_raw_abs_modulo(8) == 0 {
            let x_axis = sim.particle_rng().next_raw_abs_modulo(2) == 0;
            let delta = sim.particle_rng().next_raw_abs_modulo(3) as i32 - 1;
            if x_axis {
                p.drift_x = p.drift_x.wrapping_add(delta);
            } else {
                p.drift_y = p.drift_y.wrapping_add(delta);
            }
            p.drift_x = p.drift_x.clamp(-2, 2);
            p.drift_y = p.drift_y.clamp(-2, 2);
        }
        p.motion = [
            NativeF32Bits::POSITIVE_ZERO,
            NativeF32Bits::POSITIVE_ZERO,
            X87::store_f32_masked_chop(X87::sub(
                X87::load_i32(-2),
                X87::load_i32(rules.general.gravity),
            )),
        ];
        if sim.resolved_terrain.is_some() {
            let old = p
                .coords
                .to_array()
                .map(|n| X87::store_f32_masked_chop(X87::load_i32(n)));
            let candidate_f32 = std::array::from_fn(|i| {
                X87::store_f32_masked_chop(X87::add(
                    X87::load_f32(old[i]),
                    X87::load_f32(p.motion[i]),
                ))
            });
            let to_i32 = |bits| X87::ftol_i32_low_masked(X87::load_f32(bits));
            let motion = SparkMotionStep {
                old_coords: IVec3::from_array(old.map(to_i32)),
                candidate_coords: IVec3::from_array(candidate_f32.map(to_i32)),
                candidate_f32,
                persistent_velocity: p.motion,
                probe_velocity: p.motion,
            };
            let facts = SparkCollisionWorld::new(sim, rules)
                .and_then(|world| world.query(motion))
                .expect("Gas collision requires supported live cell facts");
            let collision = resolve_collision(motion, facts).expect("finite Gas slope reflection");
            if let Some(vector) = collision.transient_reflection {
                p.motion = vector;
            }
        }
        let height = p.coords.z.wrapping_sub(ground(sim, p.coords));
        p.drift_z = if height > 5 {
            p.drift_z.wrapping_sub(1).max(-5)
        } else {
            p.drift_z.max(0)
        };
    }
    let pt = rules.particle_type(p.type_id);
    p.damage_counter = p.damage_counter.wrapping_sub(1);
    if p.damage_counter == 0 && pt.damage != 0 {
        p.damage_counter = pt.max_dc as i16;
        apply_damage(p, sim, rules, owner_house, context);
    }
    if super::system_ai::state_advance_due(p, pt.max_ec, pt.state_ai_advance) {
        p.animation_state = p.animation_state.wrapping_add(1);
    }
    if p.animation_state == pt.end_state_ai {
        if pt.delete_on_state_limit {
            p.marked_for_deletion = true;
        } else {
            p.animation_state = 0;
        }
    }
}

fn apply_damage(
    p: &Particle,
    sim: &mut Simulation,
    rules: &RuleSet,
    owner_house: Option<InternedId>,
    context: UninitContext<'_>,
) {
    let pt = rules.particle_type(p.type_id);
    let Some(warhead) = pt.warhead.as_deref() else {
        return;
    };
    let warhead = sim.interner.intern(warhead);
    let cell = ((p.coords.x >> 8) as u16, (p.coords.y >> 8) as u16);
    let mut current = sim.cell_objects(cell, MovementLayer::Ground).next();
    while let Some(member) = current {
        //62C293 captures NextObject before its synchronous damage callback.
        let next = {
            let mut members = sim.cell_objects(cell, MovementLayer::Ground);
            members.find(|candidate| *candidate == member);
            members.next()
        };
        let coords = match member {
            CellObjectMember::Entity(id) => sim
                .entities()
                .get(id)
                .filter(|e| e.lifecycle.object_alive() && e.health.current() > 0)
                .map(|e| crate::sim::movement::ground_pose::position_world_coord(&e.position)),
            CellObjectMember::Terrain(id) => sim
                .terrain_objects
                .objects()
                .get(&id)
                .filter(|e| e.is_live() && e.health() > 0)
                .map(|e| {
                    let c = e.world_coord();
                    crate::sim::components::DriveCoord {
                        x: c.x,
                        y: c.y,
                        z: c.z,
                    }
                }),
        };
        if let Some(coords) = coords {
            let distance = adjust_for_z_standard(
                p.coords
                    .x
                    .wrapping_sub(coords.x)
                    .wrapping_abs()
                    .wrapping_add(p.coords.y.wrapping_sub(coords.y).wrapping_abs()),
            );
            match member {
                CellObjectMember::Entity(id) => {
                    sim.commit_direct_damage_receiver(
                        rules,
                        context.registry(),
                        EntityDamageEvent::direct_receiver(
                            id,
                            pt.damage,
                            distance,
                            RAD_NO_ATTACKER,
                            owner_house,
                            warhead,
                            ReceiverCallFlags {
                                ignore_defenses: false,
                                arg6: false,
                            },
                        ),
                        context.effects(),
                    );
                }
                CellObjectMember::Terrain(id) => {
                    sim.commit_direct_terrain_damage_receiver(
                        rules,
                        context.registry(),
                        TerrainDamageEvent {
                            damage_writeback: None,
                            stable_id: id,
                            rx: cell.0,
                            ry: cell.1,
                            damage: pt.damage,
                            distance_leptons: distance,
                            warhead_ref: warhead,
                            near_center_ic_isolation_eligible: false,
                        },
                        true,
                        context.effects(),
                    );
                }
            }
        }
        current = next;
    }
}

fn ground(sim: &Simulation, coords: IVec3) -> i32 {
    constructor_ground_height(sim, coords.x, coords.y)
        .expect("Gas movement requires supported ground")
        .unwrap_or(0)
}

fn move_gas(p: &mut Particle, sim: &Simulation, rules: &RuleSet) {
    let pt = rules.particle_type(p.type_id);
    let frame = sim.session.binary_frame() as i32;
    let mut coords = p.coords;
    if pt.wind_effect > 0
        && frame
            .checked_rem(10 / pt.wind_effect)
            .expect("native Gas wind requires a nonzero divisor")
            == 0
    {
        let wind = usize::try_from(rules.general.wind_direction())
            .ok()
            .filter(|&i| i < GAS_WIND_DX.len())
            .expect("native Gas wind table requires WindDirection in 0..7");
        coords.x = coords.x.wrapping_add(GAS_WIND_DX[wind]);
        coords.y = coords.y.wrapping_add(GAS_WIND_DY[wind]);
    }
    if frame & 1 != 0 {
        coords.x = X87::ftol_i32_low_masked(X87::add(
            X87::load_i32(coords.x),
            X87::load_f32(p.motion[0]),
        ));
        coords.y = X87::ftol_i32_low_masked(X87::add(
            X87::load_i32(coords.y),
            X87::load_f32(p.motion[1]),
        ));
        let floor = ground(sim, coords).wrapping_add(5);
        if coords.z > floor {
            coords.z = coords.z.wrapping_sub(coords.z.wrapping_sub(floor).min(2));
        }
        coords = offset(coords, IVec3::new(p.drift_x, p.drift_y, p.drift_z));
        coords.z = coords.z.max(ground(sim, coords).wrapping_add(5));
    }
    p.coords = coords;
}

fn offset(coords: IVec3, delta: IVec3) -> IVec3 {
    IVec3::new(
        coords.x.wrapping_add(delta.x),
        coords.y.wrapping_add(delta.y),
        coords.z.wrapping_add(delta.z),
    )
}
