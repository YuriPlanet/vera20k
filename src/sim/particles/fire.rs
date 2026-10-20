//! Fire `BehavesLike` system + particle AI.
//!
//! Per-tick fire logic for both the system (cadence-driven spawning via
//! `spawn_particle_with_insert` for ordering variety) and individual
//! particles (velocity-gated death, direction jitter, animation-state
//! translucency thresholds, decel, damage-counter bookkeeping with
//! FinalDamageState clamp).
//!
//! Fire differs from smoke and gas in three parity-critical ways:
//!   - A particle dies the instant its velocity drops to zero, not just
//!     on lifetime expiry — the flame trail truncates cleanly when a
//!     weapon stops firing.
//!   - The damage counter only resets when `animation_state` is at or
//!     below `FinalDamageState`. Past that, the counter still decrements
//!     but stops looping back to MaxDC, so faded fire stops dealing damage.
//!   - Translucency is animation-state-driven: at `Translucent25State`
//!     the byte flips to25, at `Translucent50State` to50.
//!
//! `move_fire` applies the AI-written `prev_delta` and kills the particle
//! on rising terrain (cliff death) — the canonical "flame hits a wall"
//! visual.
//!
//! ## Deferred (tracked for follow-up tasks)
//! - Damage application to cell occupants (Task C6). Counter bookkeeping and
//!   FinalDamageState gate are in place; the apply call is the only
//!   missing piece. Distance scaling (`distance/10`) and bridge-layer
//!   awareness land with C6.
//! - Wiring `move_fire` into the per-tick path — needs ground-height
//!   queries against the map. Today `move_fire` is a tested helper
//!   waiting for a caller.
//!
//! Animation uses the shared native ID/EC cadence owner. It does not read
//! SHP frame count. This change establishes no complete Fire mechanism.
//!
//! - Orbital attached-object tracking in `tick_system` — needs entity
//!   coordinate access + `RateTimer`. Today the system spawns from its
//!   fixed `sys.coords` at the SpawnFrames cadence.
//! - Attached-object alive check (mark system for deletion when its
//!   target dies). Needs an entity-alive helper.
//! - Spawn-on-target-moved bonus (3-tick fallback when target moves).

use super::spawn::spawn_particle_with_insert;
use super::{Particle, ParticleSystem};
use crate::rules::particle_type::ParticleType;
#[cfg(any(test, feature = "test-support"))]
use crate::rules::particle_type::ParticleTypeId;
use crate::rules::ruleset::RuleSet;
use crate::sim::rng::SimRng;
use crate::sim::world::Simulation;
use crate::util::fixed_math::SimFixed;
use crate::util::native_x87::{MaskedX87Chop53 as X87, MaskedX87Ordering, NativeF32Bits};

/// `SpawnParticleWithInsert` range used by fire systems — particles inserted
/// within the last 4 slots create the non-monotonic flame trail.
const FIRE_INSERT_RANGE: usize = 4;

/// FireAI62CB10 body; its common EC tail lives in AI_Dispatch62CE40.
/// Native control/address reading: tools/spatial_oracle/damage_smoke.md.
/// Fire movement/damage application is a separate incomplete mechanism.
pub(super) fn tick_particle(p: &mut Particle, pt: &ParticleType, rng: &mut SimRng) {
    if matches!(
        X87::compare(X87::load_f32(p.velocity), X87::load_i32(0)),
        MaskedX87Ordering::Less | MaskedX87Ordering::Equal
    ) {
        p.velocity = NativeF32Bits::POSITIVE_ZERO;
        p.marked_for_deletion = true;
        return; //62CB1F: no jitter, state, speed/DC writes beyond this gate.
    }
    let raw = rng.next_raw_modulo_signed(10) - 5;
    //62CB41..62CBBD: original factor uses double[7EFB40]=0.02.
    // RESIDUAL: the existing fixed direction/motion representation retains
    // quantization and incomplete movement; this does not establish Fire parity.
    let jitter = SimFixed::from_num(1) + SimFixed::from_num(raw) * SimFixed::from_num(0.02);
    p.prev_delta = p.direction.map(|component| component * jitter);
    if (p.animation_state as i8) < (pt.end_state_ai as i8)
        && super::system_ai::state_advance_due(p, pt.max_ec, p.state_ai_advance)
    {
        p.animation_state = p.animation_state.wrapping_add(1);
        //62CC1E/62CC32: exact thresholds, only on an actual increment.
        if p.animation_state == pt.translucent_25_state {
            p.translucency = 25;
        }
        if p.animation_state == pt.translucent_50_state {
            p.translucency = 50;
        }
        if p.animation_state == pt.end_state_ai && pt.delete_on_state_limit {
            p.marked_for_deletion = true;
            return; //End deletion skips decel and DC, but not shared EC tail.
        }
    }
    p.velocity =
        X87::store_f32_masked_chop(X87::sub(X87::load_f32(p.velocity), X87::load_f32(pt.deacc)));
    p.damage_counter = p.damage_counter.wrapping_sub(1);
    if p.damage_counter == 0
        && pt.damage != 0
        && (p.animation_state as i8) <= (pt.final_damage_state as i8)
    {
        p.damage_counter = pt.max_dc as i16;
        //62CCB8: occupancy damage remains a separate Fire dependency.
    }
}

/// Advance one fire `ParticleSystem` by one tick.
pub(super) fn tick_system(sys: &mut ParticleSystem, sim: &mut Simulation, rules: &RuleSet) {
    let pst = rules.particle_system_type(sys.type_id);
    let cap = pst.particle_cap as usize;
    let tick = u64::from(sim.session.binary_frame());

    // Phase 1 — tick existing particles.
    for p in &mut sys.particles {
        super::system_ai::tick_particle(p, sim, rules);
    }

    // Phase 2 — prune dead particles. Fire has no NextParticle chaining
    // (the [FireStream] type has none and no fire chain exists in vanilla).
    // Native System cleanup walks backward; each child reaches the same
    // inherited ObjectUnInit5F65F0 and common deferred drain as Spark/Smoke.
    for index in (0..sys.particles.len()).rev() {
        if sys.particles[index].marked_for_deletion {
            let particle = sys
                .remove_expired_child(sys.particles[index].stable_id())
                .expect("the current child is present");
            sim.retire_particle(particle, rules);
        }
    }

    // Phase 3 — spawn at SpawnFrames cadence via the insert-shuffle helper.
    // Orbital attached-object tracking + the target-moved 3-tick fallback
    // are deferred (see module doc).
    if !sys.done_spawning && pst.spawns {
        let frames = (pst.spawn_frames as u64).max(1);
        if tick % frames == 0 && sys.particles.len() < cap {
            let _ = spawn_particle_with_insert(sys, sys.coords, FIRE_INSERT_RANGE, rules, sim);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;

    use crate::util::fixed_math::SIM_ZERO;
    use glam::IVec3;

    fn parse(ini_text: &str) -> RuleSet {
        RuleSet::from_ini(&IniFile::from_str(ini_text)).expect("rules parse")
    }

    #[test]
    fn jitter_signed_remainder_dips_below_old_floor() {
        // Unsigned modulo would limit raw to [-5, 4], so factor0.02
        // could not reach below0.90.
        // Binary: signed (raw % 10) - 5 -> raw in [-14, 4] -> jitter can reach
        // 0.72, impossible under the non-negative draw. Drive a fixed-east
        // unit direction so prev_delta[0] == jitter, and target the seed=1 third
        // raw draw (0xDA63B931, negative): -630_998_735 % 10 = -5 -> raw = -10 ->
        // jitter = 1 + (-10)*0.02 = 0.80. Fixed motion remains a residual.
        let rules = parse(
            "[Particles]\n\
             1=Fire\n\
             [Fire]\n\
             BehavesLike=Fire\n\
             MaxEC=500\n",
        );
        let pt = rules.particle_type(ParticleTypeId(0));

        // Build the particle with a throwaway stream so the jitter draw below is
        // exactly the crafted stream's next draw.
        let mut throwaway = Simulation::with_seed(7);
        let mut p = Particle::new(
            ParticleTypeId(0),
            IVec3::ZERO,
            IVec3::ZERO,
            pt,
            &mut throwaway,
        );
        p.direction = [SimFixed::from_num(1), SIM_ZERO, SIM_ZERO];
        p.velocity = NativeF32Bits::from_bits(0x42c8_0000);

        let mut rng = SimRng::new(1);
        rng.next_u32(); // skip draw 1
        rng.next_u32(); // skip draw 2 -> next (3rd) draw is the negative 0xDA63B931
        tick_particle(&mut p, pt, &mut rng);

        // prev_delta[0] == direction[0] * jitter == jitter; must be below the
        // unsigned0.90 floor (proving the signed range), near0.80.
        assert!(
            p.prev_delta[0] < SimFixed::from_num(0.81),
            "signed jitter must reach below the unsigned floor, got {}",
            p.prev_delta[0]
        );
        assert!(
            p.prev_delta[0] > SimFixed::from_num(0.79),
            "jitter sanity lower bound"
        );
    }
}
