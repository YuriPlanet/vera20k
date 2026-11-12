//! Virus gas: the shared Scenario system, GasAI62BD50, Move62D2A0 and
//! reverse SystemAI62E6D0. Evidence: tools/spatial_oracle/virus_gas.
//!
//! The system stays in storage during damage: killing an animal can append a
//! particle synchronously. AI visits the original reverse list; the subsequent
//! Move/destruction walk includes those new births, as the original does.

use super::spark::{SparkMotionStep, resolve_collision};
use super::spark_world::{SparkCollisionWorld, SparkWorldError, constructor_ground_height};
use super::system_ai::SparkSystemTickError;
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

pub(super) fn reverse_ids(system: &ParticleSystem) -> Vec<(usize, u64)> {
    system
        .particles
        .iter()
        .enumerate()
        .rev()
        .map(|(index, p)| (index, p.stable_id()))
        .collect()
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
    let (owner_house, owner_entity) = (sys.owner_house, sys.owner_entity);
    let ids = reverse_ids(sys);
    for (index, id) in ids {
        let Some(mut p) = sim.particle_systems().particle_for_tick(system, index, id) else {
            continue;
        };
        if rules.particle_type(p.type_id).behaves_like == ParticleBehavesLike::Gas {
            if let Err(error) =
                tick_particle(&mut p, sim, rules, owner_house, owner_entity, context)
            {
                super::system_ai::retire_refused(&mut p, &error);
            }
            super::system_ai::finish_lifetime(&mut p);
        } else {
            super::system_ai::tick_particle(&mut p, sim, rules, owner_house, owner_entity);
        }
        sim.substrate
            .particle_systems
            .commit_particle_tick(system, index, p);
    }
    let Some(sys) = sim.particle_systems().get(system) else {
        return;
    };
    for (index, id) in reverse_ids(sys) {
        let Some(mut p) = sim.particle_systems().particle_for_tick(system, index, id) else {
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
        } else {
            super::system_ai::move_dispatch(&mut p, sim, rules);
            sim.substrate
                .particle_systems
                .commit_particle_tick(system, index, p);
        }
    }
}

/// Complete Gas body; the dispatcher separately owns its word-EC tail.
/// Shared Spark query/reflection owners implement the identical bridge,
/// ground/contact, building/wall and slope-matrix dependency. An `Err` is a
/// cell VERA's query refuses; the caller retires the particle.
pub(super) fn tick_particle(
    p: &mut Particle,
    sim: &mut Simulation,
    rules: &RuleSet,
    owner_house: Option<InternedId>,
    owner: Option<u64>,
    context: UninitContext<'_>,
) -> Result<(), SparkSystemTickError> {
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
            let facts = SparkCollisionWorld::new(sim, rules)?.query(motion)?;
            let collision = resolve_collision(motion, facts)?;
            if let Some(vector) = collision.transient_reflection {
                p.motion = vector;
            }
        }
        let height = p.coords.z.wrapping_sub(ground(sim, p.coords)?);
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
        apply_damage(p, sim, rules, owner_house, owner, context);
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
    Ok(())
}

/// `owner` is the system's owner (`+0xE0`); only Ares reads it here.
fn apply_damage(
    p: &Particle,
    sim: &mut Simulation,
    rules: &RuleSet,
    owner_house: Option<InternedId>,
    #[cfg_attr(not(feature = "ares"), allow(unused_variables))] owner: Option<u64>,
    context: UninitContext<'_>,
) {
    let pt = rules.particle_type(p.type_id);
    let Some(warhead) = pt.warhead.as_deref() else {
        return;
    };
    let warhead = sim.interner.intern(warhead);
    let cell = ((p.coords.x >> 8) as u16, (p.coords.y >> 8) as u16);
    #[cfg(feature = "ares")]
    {
        let range = f64::from_bits(pt.ares_damage_range.bits());
        if range > 0.0 {
            damage_in_range(p, sim, rules, owner_house, warhead, range, context);
            return;
        }
    }
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
            // Ares 3.0 ParticleClass_Update_Gas (1005CFE0, hook 0x62C2ED):
            // a Techno system owner is the attacker and its house the source.
            #[cfg(feature = "ares")]
            let (attacker, owner_house) = match owner
                .and_then(|id| sim.entities().get(id).map(|techno| (id, techno.owner())))
            {
                Some((id, house)) => (id, Some(house)),
                None => (RAD_NO_ATTACKER, owner_house),
            };
            #[cfg(not(feature = "ares"))]
            let attacker = RAD_NO_ATTACKER;
            match member {
                CellObjectMember::Entity(id) => {
                    sim.commit_direct_damage_receiver(
                        rules,
                        context.registry(),
                        EntityDamageEvent::direct_receiver(
                            id,
                            pt.damage,
                            distance,
                            attacker,
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

/// Ares 3.0 ParticleClass_Update_Gas_DamageRange (100297C0, hook 0x62C23D,
/// resuming at 0x62C313): the objects of every cell within `ceil(range)`
/// of the particle's cell, in CellSpreadEnumerator order (100294D0), whose
/// Location is within `range * 256` leptons (squared, in doubles), each once;
/// then each that is still alive with Health above zero takes the type's
/// Damage at the vanilla `|dY| + |dX|` distance adjusted for Z, with no
/// attacker and the system's owner House.
#[cfg(feature = "ares")]
fn damage_in_range(
    p: &Particle,
    sim: &mut Simulation,
    rules: &RuleSet,
    owner_house: Option<InternedId>,
    warhead: InternedId,
    range: f64,
    context: UninitContext<'_>,
) {
    let pt = rules.particle_type(p.type_id);
    let leptons = range * 256.0;
    let range_sq = leptons * leptons;
    let spread = libm::ceil(range) as i32;
    let center = (p.coords.x >> 8, p.coords.y >> 8);
    let location = |sim: &Simulation, member: CellObjectMember| match member {
        CellObjectMember::Entity(id) => sim.entities().get(id).map(|e| {
            let c = crate::sim::movement::ground_pose::position_world_coord(&e.position);
            IVec3::new(c.x, c.y, c.z)
        }),
        CellObjectMember::Terrain(id) => sim.terrain_objects.objects().get(&id).map(|e| {
            let c = e.world_coord();
            IVec3::new(c.x, c.y, c.z)
        }),
    };
    let mut items: Vec<(CellObjectMember, (u16, u16))> = Vec::new();
    for (dx, dy) in crate::sim::cell_rect::cell_spread_offsets(spread.max(0) as usize) {
        let (Ok(rx), Ok(ry)) = (
            u16::try_from(center.0 + i32::from(dx)),
            u16::try_from(center.1 + i32::from(dy)),
        ) else {
            continue;
        };
        if sim
            .resolved_terrain
            .as_ref()
            .is_none_or(|terrain| terrain.cell(rx, ry).is_none())
        {
            continue;
        }
        let members: Vec<_> = sim.cell_objects((rx, ry), MovementLayer::Ground).collect();
        for member in members {
            let Some(at) = location(sim, member) else {
                continue;
            };
            let delta = |a: i32, b: i32| f64::from(a.wrapping_sub(b));
            let (dx, dy, dz) = (
                delta(at.x, p.coords.x),
                delta(at.y, p.coords.y),
                delta(at.z, p.coords.z),
            );
            // The vector lives in a 0x100-entry stack buffer it cannot grow.
            if dz * dz + dx * dx + dy * dy <= range_sq
                && items.len() < 0x100
                && !items.iter().any(|(item, _)| *item == member)
            {
                items.push((member, (rx, ry)));
            }
        }
    }
    for (member, cell) in items {
        let alive = match member {
            CellObjectMember::Entity(id) => sim
                .entities()
                .get(id)
                .is_some_and(|e| e.lifecycle.object_alive() && e.health.current() > 0),
            CellObjectMember::Terrain(id) => sim
                .terrain_objects
                .objects()
                .get(&id)
                .is_some_and(|e| e.is_live() && e.health() > 0),
        };
        let Some(at) = location(sim, member).filter(|_| alive) else {
            continue;
        };
        let distance = adjust_for_z_standard(
            p.coords
                .y
                .wrapping_sub(at.y)
                .wrapping_abs()
                .wrapping_add(p.coords.x.wrapping_sub(at.x).wrapping_abs()),
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
}

fn ground(sim: &Simulation, coords: IVec3) -> Result<i32, SparkWorldError> {
    Ok(constructor_ground_height(sim, coords.x, coords.y)?.unwrap_or(0))
}

pub(super) fn move_gas(
    p: &mut Particle,
    sim: &Simulation,
    rules: &RuleSet,
) -> Result<(), SparkWorldError> {
    let pt = rules.particle_type(p.type_id);
    let frame = sim.session.binary_frame() as i32;
    let mut coords = p.coords;
    // gamemd divides by `10 / WindEffect` (zero above 10) and indexes its
    // eight-entry tables with `[General] WindDirection` unchecked. VERA gives
    // either value no wind, as Smoke does.
    let wind = usize::try_from(rules.general.wind_direction())
        .ok()
        .filter(|&i| i < GAS_WIND_DX.len());
    if pt.wind_effect > 0
        && frame.checked_rem(10 / pt.wind_effect) == Some(0)
        && let Some(wind) = wind
    {
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
        let floor = ground(sim, coords)?.wrapping_add(5);
        if coords.z > floor {
            coords.z = coords.z.wrapping_sub(coords.z.wrapping_sub(floor).min(2));
        }
        coords = offset(coords, IVec3::new(p.drift_x, p.drift_y, p.drift_z));
        coords.z = coords.z.max(ground(sim, coords)?.wrapping_add(5));
    }
    p.coords = coords;
    Ok(())
}

fn offset(coords: IVec3, delta: IVec3) -> IVec3 {
    IVec3::new(
        coords.x.wrapping_add(delta.x),
        coords.y.wrapping_add(delta.y),
        coords.z.wrapping_add(delta.z),
    )
}
