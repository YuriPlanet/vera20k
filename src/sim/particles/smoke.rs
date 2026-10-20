//! Ordinary Smoke, from ParticleSystemAI62ED40, ParticleAI62C540
//! and MoveSmoke62D3F0. Original evidence/control corpus:
//! tools/spatial_oracle/damage_smoke.{md,json}.
//!
//! The Particle owner retains raw float stores: the spawn accumulator changes
//! future emission/RNG frames, and speed stores change integer rise. Native
//! 53-bit/chop operations use the existing numeric owner, never host FP state.
//! Moving Techno owners retain the constructor's world offset; buildings stay
//! stationary. Foot owners in tubes suppress births, not existing particles.

use super::wind::{SMOKE_WIND_DX, SMOKE_WIND_DY};
use super::{Particle, ParticleSystem};
use crate::rules::particle_type::{ParticleBehavesLike, ParticleType};
use crate::rules::ruleset::RuleSet;
use crate::sim::rng::SimRng;
use crate::sim::world::Simulation;
use crate::util::native_x87::{MaskedX87Chop53 as X87, MaskedX87Ordering, NativeF32Bits};
use glam::IVec3;

pub(super) fn tick_system(sys: &mut ParticleSystem, sim: &mut Simulation, rules: &RuleSet) {
    let pst = rules.particle_system_type(sys.type_id);
    //62ED49..62EDB5: Techno flag2, excluding Building WhatAmI6. This is
    //before child AI even when done spawning. GetCoords has one shared owner;
    //the offset is retained in world axes, so turning does not rotate it.
    //Source/control: tools/spatial_oracle/damage_smoke.md (--following).
    if let Some(owner) = sys
        .owner_entity
        .and_then(|id| sim.substrate.entities.get(id))
        && owner.category().is_foot()
    {
        let origin = crate::sim::movement::ground_pose::object_get_coords(
            owner,
            sim.resolved_terrain.as_ref(),
        );
        sys.coords = offset_coords(
            IVec3::new(origin.x, origin.y, origin.z),
            sys.offset.x,
            sys.offset.y,
            sys.offset.z,
        );
    }
    //62EDD0: every original child AI forward, even one already marked dead.
    for p in &mut sys.particles {
        super::system_ai::tick_particle(p, sim, rules);
    }
    //62ED40/62F037: backward over the ORIGINAL count. Appended children wait until
    //the next visit; parent deletion compacts only after both births/fades.
    for index in (0..sys.particles.len()).rev() {
        if !sys.particles[index].marked_for_deletion {
            let p = &mut sys.particles[index];
            let pt = rules.particle_type(p.type_id);
            if pt.behaves_like == ParticleBehavesLike::Smoke {
                move_smoke(p, pt, rules.general.wind_direction(), sim);
            }
            // RESIDUAL: mixed Gas/Fire MoveDispatch bodies remain incomplete;
            // neither stock damaged-building Smoke type has NextParticle.
            continue; // a Move-dead child is retained/drawn until next visit.
        }
        let parent = sys.particles[index].clone();
        let pt = rules.particle_type(parent.type_id);
        if let Some(next) = pt.next_particle {
            let radius = pt.radius >> 3;
            let dx = symmetric_offset(radius, sim.particle_rng());
            let dy = symmetric_offset(radius, sim.particle_rng());
            for sign in [1i32, -1] {
                let coords = offset_coords(
                    parent.coords,
                    dx.wrapping_mul(sign),
                    dy.wrapping_mul(sign),
                    0,
                );
                let mut child =
                    Particle::new(next, coords, IVec3::ZERO, rules.particle_type(next), sim);
                //62EF3B/62EFA0..62F003: ctor Smoke draw is retained even though its
                //speed is overwritten; one fade draw follows EACH ctor.
                child.velocity = parent.velocity;
                child.translucency = if sim.particle_rng().next_raw_modulo_signed(6) != 0 {
                    parent.translucency.wrapping_add(25)
                } else {
                    parent.translucency
                };
                sys.particles.push(child);
            }
        }
        let particle = sys
            .remove_expired_child(parent.stable_id())
            .expect("the current child is present");
        sim.retire_particle(particle, rules);
    }

    //62F047: Smoke ignores Spawns and ParticleCap. Offset draws precede
    //the HoldsWhat lookup, even when the type index is absent.
    if !sys.done_spawning && sys.alive() {
        let divisor = X87::ftol_i32_low_masked(X87::load_f32(sys.spawn_timer));
        if (sim.session.binary_frame() as i32)
            .checked_rem(divisor)
            .expect("native Smoke spawning requires valid IDIV")
            == 0
            //62F080..62F098: Foot flag4 with signed tube byte+684 >= 0
            //skips emission BEFORE its offset/constructor RNG draws. Existing
            //particles, spawn accumulator and system lifetime still advance.
            && !sys
                .owner_entity
                .and_then(|id| sim.substrate.entities.get(id))
                .is_some_and(|owner| {
                    owner.category().is_foot() && owner.low_bridge_tube_state.is_some()
                })
        {
            let radius = pst.spawn_radius.wrapping_add(1);
            assert_ne!(radius, 0, "native Smoke spawning divides by SpawnRadius+1");
            let dx = sim.particle_rng().next_raw_modulo_signed(radius as u32);
            let dy = sim.particle_rng().next_raw_modulo_signed(radius as u32);
            if let Some(holds) = pst.holds_what {
                let coords = offset_coords(sys.coords, dx, dy, 10);
                let mut p = Particle::new(
                    holds,
                    coords,
                    sys.target_coords,
                    rules.particle_type(holds),
                    sim,
                );
                if X87::compare(
                    X87::load_f32(sys.spawn_timer),
                    X87::load_f32(pst.spawn_translucency_cutoff),
                ) == MaskedX87Ordering::Greater
                {
                    p.translucency = p.translucency.wrapping_add(25);
                }
                //62F198..62F1D6, constant [7EFB50]=stored 0.35.
                let speed = X87::sub(
                    X87::load_f32(p.velocity),
                    X87::mul(
                        X87::sub(
                            X87::load_f32(sys.spawn_timer),
                            X87::load_i32(pst.spawn_frames),
                        ),
                        X87::load_f32(NativeF32Bits::from_bits(0x3EB3_3333)),
                    ),
                );
                p.velocity = X87::store_f32_masked_chop(speed);
                if X87::compare(speed, X87::load_i32(2)) == MaskedX87Ordering::Less {
                    p.velocity = NativeF32Bits::from_bits(0x4000_0000);
                }
                sys.particles.push(p);
            }
        }
    }
    //62F1F3..62F218: cutoff compares the UNSTORED sum. It can exceed
    //cutoff although the chopped binary32 result equals cutoff exactly.
    let sum = X87::add(X87::load_f32(sys.spawn_timer), X87::load_f32(pst.slowdown));
    sys.spawn_timer = X87::store_f32_masked_chop(sum);
    if X87::compare(sum, X87::load_f32(pst.spawn_cutoff)) == MaskedX87Ordering::Greater {
        sys.done_spawning = true;
    }
}

/// SmokeAI62C540 body; the shared dispatcher owns the subsequent EC tail.
pub(super) fn tick_particle(p: &mut Particle, pt: &ParticleType, frame: u32, rng: &mut SimRng) {
    if frame & 1 != 0 {
        if (rng.next_u32() as i32).wrapping_abs() & 3 == 0 {
            let axis = (rng.next_u32() as i32).wrapping_abs() & 1;
            // Native wraps ABS BEFORE signed %3. INT_MIN therefore yields-3.
            let step = (rng.next_u32() as i32).wrapping_abs() % 3 - 1;
            let drift = if axis == 0 {
                &mut p.drift_x
            } else {
                &mut p.drift_y
            };
            *drift = drift.wrapping_add(step).clamp(-5, 5);
        }
        p.drift_z = 0;
    }
    if (p.animation_state as i8) < (pt.end_state_ai as i8)
        && super::system_ai::state_advance_due(p, pt.max_ec, pt.state_ai_advance)
    {
        p.animation_state = p.animation_state.wrapping_add(1);
        if p.animation_state == pt.end_state_ai && pt.delete_on_state_limit {
            p.marked_for_deletion = true;
        }
    }
    if X87::compare(X87::load_f32(p.velocity), X87::load_i32(3)) == MaskedX87Ordering::Greater {
        p.velocity = X87::store_f32_masked_chop(X87::sub(
            X87::load_f32(p.velocity),
            X87::load_f32(pt.deacc),
        ));
    }
}

/// MoveSmoke62D3F0: old-cell bridge admission precedes raw-coordinate commit.
pub(super) fn move_smoke(p: &mut Particle, pt: &ParticleType, wind: i32, sim: &Simulation) {
    let index = usize::try_from(wind)
        .ok()
        .filter(|&i| i < SMOKE_WIND_DX.len())
        .expect("native Smoke wind table requires WindDirection in 0..7");
    let rise = X87::ftol_i32_low_masked(X87::load_f32(p.velocity)).wrapping_add(p.drift_z);
    let next = offset_coords(
        p.coords,
        SMOKE_WIND_DX[index]
            .wrapping_mul(pt.wind_effect)
            .wrapping_add(p.drift_x),
        SMOKE_WIND_DY[index]
            .wrapping_mul(pt.wind_effect)
            .wrapping_add(p.drift_y),
        rise,
    );
    let deck = super::spark_world::smoke_bridge_deck(sim, p.coords)
        .expect("Smoke movement requires supported old-cell ground facts");
    if deck.is_some_and(|deck| {
        p.coords.z < deck
            && next.z >= deck.wrapping_sub(crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS * 5 / 2)
    }) {
        p.marked_for_deletion = true;
    } else {
        p.coords = next;
    }
}

fn offset_coords(p: IVec3, x: i32, y: i32, z: i32) -> IVec3 {
    IVec3::new(
        p.x.wrapping_add(x),
        p.y.wrapping_add(y),
        p.z.wrapping_add(z),
    )
}

fn symmetric_offset(radius: i32, rng: &mut SimRng) -> i32 {
    assert_ne!(radius, 0, "native Smoke child split divides by Radius>>3");
    let offset = rng.next_raw_modulo_signed(radius as u32);
    if offset <= 0 {
        offset.wrapping_sub(radius)
    } else {
        offset.wrapping_add(radius)
    }
}

#[cfg(test)]
#[path = "smoke_native_tests.rs"]
mod native_tests;
