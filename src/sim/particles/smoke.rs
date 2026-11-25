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
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::particle_system_type::ParticleSystemType;
use crate::rules::particle_type::{ParticleBehavesLike, ParticleType};
use crate::rules::ruleset::RuleSet;
use crate::sim::rng::SimRng;
use crate::sim::world::Simulation;
use crate::util::forked::Forked;
use crate::util::native_x87::{MaskedX87Chop53 as X87, MaskedX87Ordering, NativeF32Bits};
use glam::IVec3;

pub(super) fn tick_system(sim: &mut Simulation, rules: &RuleSet, id: u64) {
    let Some(sys) = sim.particle_systems().get(id) else {
        return;
    };
    let pst = rules.particle_system_type(sys.type_id);
    let (owner_house, owner_entity) = (sys.owner_house, sys.owner_entity);
    // ParticleSystemAI62ED40 / ParticleAIDispatch62CE40: child Gas damage
    // calls ReceiveDamage synchronously. Keep this registered system resident
    // while that callback inserts another Ground system and reads GetYSort.
    //62ED49..62EDB5: follow an Object owner that is not a Building (a Foot, or
    // the VoxelAnim an AttachedSystem rides, 0x007498A5) before any child AI,
    // including when done spawning. Publish the new coordinates to the same
    // store first.
    let cells = sim
        .resolved_terrain
        .as_ref()
        .map(crate::map::resolved_terrain::NativeCellQuery::canonical);
    let mut followed = sys
        .owner_entity
        .filter(|&owner| {
            sim.substrate
                .entities
                .get(owner)
                .is_none_or(|owner| owner.category().is_foot())
        })
        .and_then(|owner| {
            crate::sim::movement::ground_pose::target_get_coords(
                crate::sim::combat::TargetKind::Entity(owner),
                sim,
                cells.as_ref(),
            )
        })
        .map(|origin| {
            offset_coords(
                IVec3::new(origin.x, origin.y, origin.z),
                sys.offset.x,
                sys.offset.y,
                sys.offset.z,
            )
        });
    let frame = sim.session.binary_frame();
    //62EDC7..62EDDE re-reads Count after each synchronous child AI.
    // VERA-only, results unchanged: the system is found once for each run of
    // Smoke children, whose AI reads nothing of the world and runs on the
    // child where it is stored. Any other child's AI may reach the whole
    // world (Gas damage can append to this system), so it runs on a copy
    // that the store takes back, and the system is found again after it.
    let mut index = 0;
    let count = loop {
        let Some((sys, rng)) = sim
            .substrate
            .particle_systems
            .system_and_rng_mut(id, &mut sim.scenario_rng)
        else {
            return;
        };
        if let Some(coords) = followed.take() {
            sys.coords = coords;
        }
        while let Some(p) = sys.particles.get_mut(index) {
            let pt = rules.particle_type(p.type_id);
            if pt.behaves_like != ParticleBehavesLike::Smoke {
                break;
            }
            super::system_ai::tick_smoke_particle(p, pt, frame, rng);
            index += 1;
        }
        let Some(mut p) = sys.particles.get(index).cloned() else {
            break sys.particles.len();
        };
        super::system_ai::tick_particle(&mut p, sim, rules, owner_house, owner_entity);
        sim.substrate
            .particle_systems
            .commit_particle_tick(id, index, p);
        index += 1;
    };
    //62EDE0 freezes the reverse walk's initial index: newly appended children
    //wait for their next AI visit; parent removal follows both births/fades.
    super::system_ai::move_or_destroy_children(sim, rules, id, count, |sim, p| {
        let pt = rules.particle_type(p.type_id);
        if let Some(next) = pt.next_particle {
            let radius = pt.radius >> 3;
            let dx = symmetric_offset(radius, sim.particle_rng());
            let dy = symmetric_offset(radius, sim.particle_rng());
            for sign in [1i32, -1] {
                let coords =
                    offset_coords(p.coords, dx.wrapping_mul(sign), dy.wrapping_mul(sign), 0);
                let mut child = Particle::new(next, coords, IVec3::ZERO, rules, sim);
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
    });

    //62F047: Smoke ignores Spawns and ParticleCap. Offset draws precede
    //the HoldsWhat lookup, even when the type index is absent. The owner's
    //release of +0x304 comes before this read of the done byte.
    let substrate = &mut sim.substrate;
    let Some(sys) = substrate.particle_systems.get_mut(id) else {
        return;
    };
    sys.apply_fire_link_release(&substrate.entities);
    let (done, alive, timer, owner, coords, target_coords) = (
        sys.done_spawning,
        sys.alive(),
        sys.spawn_timer,
        sys.owner_entity,
        sys.coords,
        sys.target_coords,
    );
    // gamemd traps on a zero period (`SpawnFrames=0`, or a negative
    // `Slowdown=` walking the timer through zero); VERA spawns nothing
    // that frame and the timer still moves on.
    let due = !done
        && alive
        && (frame as i32).checked_rem(X87::ftol_i32_low_masked(X87::load_f32(timer))) == Some(0);
    if !due {
        // No birth is due, so nothing stands between this read and the
        // timer's store: the system at hand takes it.
        advance_spawn_timer(sys, pst, timer);
        return;
    }
    //62F080..62F098: the tube test skips emission BEFORE its
    //offset/constructor RNG draws. Existing particles, spawn
    //accumulator and system lifetime still advance.
    if !owner_in_tube(sim, owner) {
        // gamemd traps on `SpawnRadius=-1`; the helper's zero modulus
        // makes VERA spawn at the system's own X and Y, with no draw.
        let radius = pst.spawn_radius.wrapping_add(1);
        let dx = sim.particle_rng().next_raw_modulo_signed(radius as u32);
        let dy = sim.particle_rng().next_raw_modulo_signed(radius as u32);
        if let Some(holds) = pst.holds_what {
            let coords = offset_coords(coords, dx, dy, 10);
            let mut p = Particle::new(holds, coords, target_coords, rules, sim);
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
    if let Some(sys) = sim.substrate.particle_systems.get_mut(id) {
        advance_spawn_timer(sys, pst, timer);
    }
}

/// 62F1F3..62F218: the spawn accumulator (`+0xE8`) read as `timer` gains
/// Slowdown. The cutoff compares the UNSTORED sum: it can exceed the cutoff
/// although the chopped binary32 result equals the cutoff exactly.
fn advance_spawn_timer(
    sys: &mut super::ParticleSystem,
    pst: &ParticleSystemType,
    timer: NativeF32Bits,
) {
    let sum = X87::add(X87::load_f32(timer), X87::load_f32(pst.slowdown));
    sys.spawn_timer = X87::store_f32_masked_chop(sum);
    if X87::compare(sum, X87::load_f32(pst.spawn_cutoff)) == MaskedX87Ordering::Greater {
        sys.done_spawning = true;
    }
}

/// The smoke spawn's tube test (`0x0062F080..0x0062F098`, and Ares' UpdateSmoke
/// `10027DAA..10027E02`): the owner is a Foot (flag 4) whose signed tube byte
/// `+0x684` is not negative.
pub(super) fn owner_in_tube(sim: &Simulation, owner: Option<u64>) -> bool {
    owner
        .and_then(|id| sim.substrate.entities.get(id))
        .is_some_and(|owner| owner.category().is_foot() && owner.active_tube().is_some())
}

/// SmokeAI62C540 body; the shared dispatcher owns the subsequent EC tail.
pub(super) fn tick_particle(
    p: &mut Particle,
    pt: &ParticleType,
    frame: u32,
    rng: &mut Forked<SimRng>,
) {
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
/// The terrain is all it reads of the world.
pub(super) fn move_smoke(
    p: &mut Particle,
    pt: &ParticleType,
    wind: i32,
    terrain: Option<&ResolvedTerrainGrid>,
) -> Result<(), super::spark_world::SparkWorldError> {
    // MoveSmoke62D3F0 indexes its eight-entry tables with `[General]
    // WindDirection` unchecked, so a missing key (-1) or a value past 7 reads
    // whatever lies beside them. VERA gives such a value no wind.
    let (wind_dx, wind_dy) = usize::try_from(wind)
        .ok()
        .filter(|&i| i < SMOKE_WIND_DX.len())
        .map_or((0, 0), |i| (SMOKE_WIND_DX[i], SMOKE_WIND_DY[i]));
    let rise = X87::ftol_i32_low_masked(X87::load_f32(p.velocity)).wrapping_add(p.drift_z);
    let next = offset_coords(
        p.coords,
        wind_dx.wrapping_mul(pt.wind_effect).wrapping_add(p.drift_x),
        wind_dy.wrapping_mul(pt.wind_effect).wrapping_add(p.drift_y),
        rise,
    );
    let deck = super::spark_world::smoke_bridge_deck(terrain, p.coords)?;
    if deck.is_some_and(|deck| {
        p.coords.z < deck
            && next.z >= deck.wrapping_sub(crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS * 5 / 2)
    }) {
        p.marked_for_deletion = true;
    } else {
        p.coords = next;
    }
    Ok(())
}

fn offset_coords(p: IVec3, x: i32, y: i32, z: i32) -> IVec3 {
    IVec3::new(
        p.x.wrapping_add(x),
        p.y.wrapping_add(y),
        p.z.wrapping_add(z),
    )
}

/// gamemd traps on a zero radius (`Radius=` 0..7); the helper's zero modulus
/// makes VERA split both children at the parent's X and Y, with no draw.
fn symmetric_offset(radius: i32, rng: &mut SimRng) -> i32 {
    let offset = rng.next_raw_modulo_signed(radius as u32);
    if offset <= 0 {
        offset.wrapping_sub(radius)
    } else {
        offset.wrapping_add(radius)
    }
}

#[cfg(all(test, tests_sim_p_s))]
#[path = "smoke_native_tests.rs"]
mod native_tests;
