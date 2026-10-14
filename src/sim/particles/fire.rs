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
#[cfg(test)]
use crate::rules::particle_type::ParticleTypeId;
use crate::rules::ruleset::RuleSet;
use crate::sim::rng::SimRng;
use crate::sim::world::Simulation;
use crate::util::fixed_math::SimFixed;
use crate::util::native_x87::{MaskedX87Chop53 as X87, MaskedX87Ordering, NativeF32Bits};
#[cfg(test)]
use glam::IVec3;

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

/// Apply fire movement to one particle.
///
/// Adds the AI-written `prev_delta` to coords, then kills the particle
/// if the new position lands on rising terrain (cliff death). Caller
/// supplies pre-queried ground heights; the actual map query is the
/// deferred per-tick wiring.
///
/// On cliff death the particle still advances to `new_coords` — that
/// matches the binary's `Move_Dispatch` which calls `SetCoords(new_pos)`
/// unconditionally after marking the particle dead. The dying particle
/// renders one frame at the cliff cell, then gets pruned next tick.
///
/// `move_fire` is a no-op when the particle's velocity has already
/// dropped to zero (fire AI's velocity gate would have marked it for
/// deletion this tick anyway, but `move_fire` may be called standalone
/// in tests or in the eventual wiring sequence).
///
/// Bridge-layer interaction is deferred to C6 — fire particles pass
/// through bridges in the binary too (no bridge check in fire move).
// Ground-height ownership is not wired into the live particle tick yet.
#[cfg(test)]
pub(super) fn move_fire(p: &mut Particle, old_ground: i32, new_ground: i32) {
    if matches!(
        X87::compare(X87::load_f32(p.velocity), X87::load_i32(0)),
        MaskedX87Ordering::Less | MaskedX87Ordering::Equal
    ) {
        return;
    }
    let dx = p.prev_delta[0].to_num::<i32>();
    let dy = p.prev_delta[1].to_num::<i32>();
    let dz = p.prev_delta[2].to_num::<i32>();
    let new_coords = p.coords + IVec3::new(dx, dy, dz);
    if old_ground < new_ground {
        // Cliff death — terrain rises, particle hits ground.
        p.marked_for_deletion = true;
        // Coords still advance — the binary's SetCoords runs after the kill.
    }
    p.coords = new_coords;
}

/// Advance one fire `ParticleSystem` by one tick.
pub(super) fn tick_system(sys: &mut ParticleSystem, sim: &mut Simulation, rules: &RuleSet) {
    let pst = rules.particle_system_type(sys.type_id);
    let cap = pst.particle_cap as usize;
    let tick = u64::from(sim.session.binary_frame);

    // Phase 1 — tick existing particles.
    for p in &mut sys.particles {
        super::system_ai::tick_particle(p, sim, rules);
    }

    // Phase 2 — prune dead particles. Fire has no NextParticle chaining
    // (the [FireStream] type has none and no fire chain exists in vanilla).
    for p in &sys.particles {
        if p.marked_for_deletion {
            sim.conceal_particle(p.stable_id());
        }
    }
    sys.particles.retain(|p| !p.marked_for_deletion);

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
    use crate::rules::particle_system_type::ParticleSystemTypeId;
    use crate::sim::particles::ParticleSystem;
    use crate::util::fixed_math::SIM_ZERO;
    use glam::IVec3;

    fn fake_system(type_id: ParticleSystemTypeId) -> ParticleSystem {
        let mut system = ParticleSystem::test_fixture(0, type_id, IVec3::ZERO);
        system.spawn_timer = NativeF32Bits::ONE;
        system
    }

    fn parse(ini_text: &str) -> RuleSet {
        RuleSet::from_ini(&IniFile::from_str(ini_text)).expect("rules parse")
    }

    #[test]
    fn velocity_zero_marks_for_deletion() {
        // Parity rule: fire dies the instant its momentum runs out.
        let rules = parse(
            "[Particles]\n\
             1=Fire\n\
             [Fire]\n\
             BehavesLike=Fire\n\
             MaxEC=500\n",
        );
        let pt = rules.particle_type(ParticleTypeId(0));
        let mut sim = Simulation::new();
        let mut p = Particle::new(ParticleTypeId(0), IVec3::ZERO, IVec3::ZERO, pt, &mut sim);
        p.velocity = NativeF32Bits::POSITIVE_ZERO;
        tick_particle(&mut p, pt, sim.particle_rng());
        assert!(p.marked_for_deletion, "zero-velocity fire dies immediately");
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

    #[test]
    fn final_damage_state_clamps_damage_counter_reset() {
        // Past FinalDamageState, the counter still decrements but stops
        // resetting — fire visibly fades but does no damage.
        let rules = parse(
            "[Particles]\n\
             1=Fire\n\
             [Fire]\n\
             BehavesLike=Fire\n\
             MaxEC=500\n\
             MaxDC=3\n\
             Velocity=28.0\n\
             StartStateAI=20\n\
             EndStateAI=99\n\
             FinalDamageState=14\n",
        );
        let pt = rules.particle_type(ParticleTypeId(0));
        let mut sim = Simulation::new();
        let mut p = Particle::new(ParticleTypeId(0), IVec3::ZERO, IVec3::ZERO, pt, &mut sim);
        // Force animation_state past final_damage_state (default 14).
        p.animation_state = 20;
        // Drive damage_counter to zero — must NOT reset to MaxDC.
        for _ in 0..5 {
            tick_particle(&mut p, pt, sim.particle_rng());
        }
        assert!(
            p.damage_counter <= 0,
            "past FinalDamageState, counter must NOT reset (got {})",
            p.damage_counter
        );
    }

    #[test]
    fn move_fire_marks_dead_when_terrain_rises() {
        // Cliff death: old_ground < new_ground → marked dead.
        // Coords still advance (binary's Move_Dispatch does SetCoords after
        // the kill); the dying particle renders one frame at the cliff cell.
        let rules = parse(
            "[Particles]\n\
             1=Fire\n\
             [Fire]\n\
             BehavesLike=Fire\n\
             MaxEC=500\n\
             Velocity=28.0\n",
        );
        let pt = rules.particle_type(ParticleTypeId(0));
        let mut sim = Simulation::new();
        let mut p = Particle::new(
            ParticleTypeId(0),
            IVec3::new(100, 100, 0),
            IVec3::ZERO,
            pt,
            &mut sim,
        );
        p.prev_delta = [SimFixed::from_num(5), SIM_ZERO, SIM_ZERO];
        // old_ground=0, new_ground=10 → terrain rises.
        move_fire(&mut p, 0, 10);
        assert!(p.marked_for_deletion, "cliff death marks for deletion");
        // Coords advance to the cliff cell — matches binary parity.
        assert_eq!(p.coords, IVec3::new(105, 100, 0));
    }

    #[test]
    fn move_fire_advances_on_flat_ground() {
        // Sanity counterpart: equal grounds → coords advance, no death.
        let rules = parse(
            "[Particles]\n\
             1=Fire\n\
             [Fire]\n\
             BehavesLike=Fire\n\
             MaxEC=500\n\
             Velocity=28.0\n",
        );
        let pt = rules.particle_type(ParticleTypeId(0));
        let mut sim = Simulation::new();
        let mut p = Particle::new(
            ParticleTypeId(0),
            IVec3::new(100, 100, 0),
            IVec3::ZERO,
            pt,
            &mut sim,
        );
        p.prev_delta = [SimFixed::from_num(5), SIM_ZERO, SIM_ZERO];
        move_fire(&mut p, 0, 0);
        assert!(!p.marked_for_deletion);
        assert_eq!(p.coords, IVec3::new(105, 100, 0));
    }

    #[test]
    fn fire_spawn_cap_enforced() {
        // Cap=3 — even with aggressive cadence, particle count must stay ≤ 3.
        let rules = parse(
            "[Particles]\n\
             1=Fire\n\
             [Fire]\n\
             BehavesLike=Fire\n\
             MaxEC=1000\n\
             Velocity=28.0\n\
             [ParticleSystems]\n\
             1=Sys\n\
             [Sys]\n\
             BehavesLike=Fire\n\
             HoldsWhat=Fire\n\
             ParticleCap=3\n\
             SpawnFrames=1\n\
             Spawns=yes\n",
        );
        let mut sim = Simulation::new();
        let mut sys = fake_system(ParticleSystemTypeId(0));
        for _ in 0..50 {
            tick_system(&mut sys, &mut sim, &rules);
            sim.session.binary_frame = sim.session.binary_frame.wrapping_add(1);
        }
        assert!(
            sys.particles.len() <= 3,
            "cap exceeded: {}",
            sys.particles.len()
        );
    }
}
