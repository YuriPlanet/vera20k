//! Ordinary Smoke, from ParticleSystemAI62ED40, ParticleAI62C540
//! and MoveSmoke62D3F0. Original evidence/control corpus:
//! tools/spatial_oracle/damage_smoke.{md,json}.
//!
//! The Particle owner retains raw float stores: the spawn accumulator changes
//! future emission/RNG frames, and speed stores change integer rise. Native
//! 53-bit/chop operations use the existing numeric owner, never host FP state.
//! Moving Techno owners retain the constructor's world offset; buildings stay
//! stationary. Foot owners in tubes suppress births, not existing particles.

use super::Particle;
use super::wind::{SMOKE_WIND_DX, SMOKE_WIND_DY};
use crate::rules::particle_type::{ParticleBehavesLike, ParticleType};
use crate::rules::ruleset::RuleSet;
use crate::sim::rng::SimRng;
use crate::sim::world::Simulation;
use crate::util::native_x87::{MaskedX87Chop53 as X87, MaskedX87Ordering, NativeF32Bits};
use glam::IVec3;

pub(super) fn tick_system(sim: &mut Simulation, rules: &RuleSet, id: u64) {
    let Some(sys) = sim.particle_systems().get(id) else {
        return;
    };
    let pst = rules.particle_system_type(sys.type_id);
    let owner_house = sys.owner_house;
    // ParticleSystemAI62ED40 / ParticleAIDispatch62CE40: child Gas damage
    // calls ReceiveDamage synchronously. Keep this registered system resident
    // while that callback inserts another Ground system and reads GetYSort.
    //62ED49..62EDB5: follow the Foot owner before any child AI, including when
    //done spawning. Publish the new coordinates to the same store first.
    let followed = sys
        .owner_entity
        .and_then(|owner| sim.substrate.entities.get(owner))
        .filter(|owner| owner.category().is_foot())
        .map(|owner| {
            let origin = crate::sim::movement::ground_pose::object_get_coords(
                owner,
                sim.resolved_terrain.as_ref(),
            );
            offset_coords(
                IVec3::new(origin.x, origin.y, origin.z),
                sys.offset.x,
                sys.offset.y,
                sys.offset.z,
            )
        });
    if let Some(coords) = followed {
        sim.substrate.particle_systems.get_mut(id).unwrap().coords = coords;
    }
    //62EDC7..62EDDE re-reads Count after each synchronous child AI.
    let mut index = 0;
    while let Some(mut p) = sim
        .particle_systems()
        .get(id)
        .and_then(|sys| sys.particles.get(index))
        .cloned()
    {
        super::system_ai::tick_particle(&mut p, sim, rules, owner_house);
        sim.substrate
            .particle_systems
            .commit_particle_tick(id, index, p);
        index += 1;
    }
    let Some(sys) = sim.particle_systems().get(id) else {
        return;
    };
    //62EDE0 freezes the reverse walk's initial index: newly appended children
    //wait for their next AI visit; parent removal follows both births/fades.
    for index in (0..sys.particles.len()).rev() {
        let Some(mut p) = sim
            .particle_systems()
            .get(id)
            .and_then(|sys| sys.particles.get(index))
            .cloned()
        else {
            continue;
        };
        let pt = rules.particle_type(p.type_id);
        if !p.marked_for_deletion {
            if pt.behaves_like == ParticleBehavesLike::Smoke {
                move_smoke(&mut p, pt, rules.general.wind_direction(), sim);
            }
            // RESIDUAL: mixed Gas/Fire MoveDispatch bodies remain incomplete.
            sim.substrate
                .particle_systems
                .commit_particle_tick(id, index, p);
            continue;
        }
        if let Some(next) = pt.next_particle {
            let radius = pt.radius >> 3;
            let dx = symmetric_offset(radius, sim.particle_rng());
            let dy = symmetric_offset(radius, sim.particle_rng());
            for sign in [1i32, -1] {
                let coords =
                    offset_coords(p.coords, dx.wrapping_mul(sign), dy.wrapping_mul(sign), 0);
                let mut child =
                    Particle::new(next, coords, IVec3::ZERO, rules.particle_type(next), sim);
                //62EF3B/62EFA0..62F003: constructor draw then one fade draw.
                child.velocity = p.velocity;
                child.translucency = if sim.particle_rng().next_raw_modulo_signed(6) != 0 {
                    p.translucency.wrapping_add(25)
                } else {
                    p.translucency
                };
                if let Some(sys) = sim.substrate.particle_systems.get_mut(id) {
                    sys.particles.push(child);
                }
            }
        }
        if let Some(particle) = sim
            .substrate
            .particle_systems
            .get_mut(id)
            .and_then(|sys| sys.remove_expired_child(p.stable_id()))
        {
            sim.retire_particle(particle, rules);
        }
    }

    //62F047: Smoke ignores Spawns and ParticleCap. Offset draws precede
    //the HoldsWhat lookup, even when the type index is absent.
    let Some(sys) = sim.particle_systems().get(id) else {
        return;
    };
    let (done, alive, timer, owner, coords, target_coords) = (
        sys.done_spawning,
        sys.alive(),
        sys.spawn_timer,
        sys.owner_entity,
        sys.coords,
        sys.target_coords,
    );
    if !done && alive {
        let divisor = X87::ftol_i32_low_masked(X87::load_f32(timer));
        if (sim.session.binary_frame() as i32)
            .checked_rem(divisor)
            .expect("native Smoke spawning requires valid IDIV")
            == 0
            //62F080..62F098: Foot flag4 with signed tube byte+684 >= 0
            //skips emission BEFORE its offset/constructor RNG draws. Existing
            //particles, spawn accumulator and system lifetime still advance.
            && !owner
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
                let coords = offset_coords(coords, dx, dy, 10);
                let mut p = Particle::new(
                    holds,
                    coords,
                    target_coords,
                    rules.particle_type(holds),
                    sim,
                );
                if X87::compare(
                    X87::load_f32(timer),
                    X87::load_f32(pst.spawn_translucency_cutoff),
                ) == MaskedX87Ordering::Greater
                {
                    p.translucency = p.translucency.wrapping_add(25);
                }
                //62F198..62F1D6, constant [7EFB50]=stored 0.35.
                let speed = X87::sub(
                    X87::load_f32(p.velocity),
                    X87::mul(
                        X87::sub(X87::load_f32(timer), X87::load_i32(pst.spawn_frames)),
                        X87::load_f32(NativeF32Bits::from_bits(0x3EB3_3333)),
                    ),
                );
                p.velocity = X87::store_f32_masked_chop(speed);
                if X87::compare(speed, X87::load_i32(2)) == MaskedX87Ordering::Less {
                    p.velocity = NativeF32Bits::from_bits(0x4000_0000);
                }
                if let Some(sys) = sim.substrate.particle_systems.get_mut(id) {
                    sys.particles.push(p);
                }
            }
        }
    }
    //62F1F3..62F218: cutoff compares the UNSTORED sum. It can exceed
    //cutoff although the chopped binary32 result equals cutoff exactly.
    let sum = X87::add(X87::load_f32(timer), X87::load_f32(pst.slowdown));
    if let Some(sys) = sim.substrate.particle_systems.get_mut(id) {
        sys.spawn_timer = X87::store_f32_masked_chop(sum);
        if X87::compare(sum, X87::load_f32(pst.spawn_cutoff)) == MaskedX87Ordering::Greater {
            sys.done_spawning = true;
        }
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
