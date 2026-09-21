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
//!   - Translucency is animation-state-driven: at `Translucent50State`
//!     the byte flips to 0x19, at `Translucent25State` to 0x32.
//!
//! `move_fire` applies the AI-written `prev_delta` and kills the particle
//! on rising terrain (cliff death) — the canonical "flame hits a wall"
//! visual.
//!
//! ## Deferred (tracked for follow-up tasks)
//! - Damage application to cell occupants (Task C6). Counter bookkeeping
//!   + FinalDamageState gate are in place; the apply call is the only
//!   missing piece. Distance scaling (`distance/10`) and bridge-layer
//!   awareness land with C6.
//! - Wiring `move_fire` into the per-tick path — needs ground-height
//!   queries against the map. Today `move_fire` is a tested helper
//!   waiting for a caller.
//! - Animation-state auto-advance — the binary's formula is
//!   `frame_ticks % ((total_frames % 2 + 1) + StateAIAdvance) == 0`,
//!   where `total_frames` comes from `GetImageFrameCount()` on the SHP
//!   asset. `sim/` can't reach the asset layer; gas/smoke defer this
//!   too. The threshold-byte mapping below works correctly once
//!   something external advances `animation_state`.
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
use crate::util::fixed_math::{SIM_ZERO, SimFixed};
#[cfg(test)]
use glam::IVec3;

/// `SpawnParticleWithInsert` range used by fire systems — particles inserted
/// within the last 4 slots create the non-monotonic flame trail.
const FIRE_INSERT_RANGE: usize = 4;

#[cfg(test)]
fn make_particle(
    type_id: ParticleTypeId,
    coords: IVec3,
    spawn_origin: IVec3,
    pt: &ParticleType,
    rng: &mut SimRng,
) -> Particle {
    let base = (pt.max_ec as u32).max(1);
    let lifetime_extra = rng.next_raw_abs_modulo(base) as i16;
    let lifetime_remaining = (pt.max_ec as i16).saturating_add(lifetime_extra);
    Particle {
        type_id,
        coords,
        previous_coords: spawn_origin,
        origin: coords,
        direction: [SIM_ZERO; 3],
        velocity: pt.velocity,
        lifetime_remaining,
        damage_counter: pt.max_dc as i16,
        state_ai_advance: pt.state_ai_advance,
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
        prev_delta: [SIM_ZERO; 3],
        state_advance_counter: 0,
    }
}

/// Per-tick AI for one fire particle.
///
/// Order: state-AI advance (animation_state + translucency-state byte writes)
/// → velocity gate → direction jitter → lifetime → decel → damage counter
/// (gated by FinalDamageState).
///
/// Damage application to cell occupants is deferred to Task C6; this
/// function only does the counter bookkeeping.
pub(super) fn tick_particle(
    p: &mut Particle,
    pt: &ParticleType,
    image_frame_count: u16,
    rng: &mut SimRng,
) {
    super::system_ai::advance_state(p, pt, image_frame_count);

    // Velocity-zero death: fire dies the instant its momentum runs out.
    if p.velocity <= SIM_ZERO {
        p.marked_for_deletion = true;
        return;
    }

    // Direction jitter: signed remainder in [-9, 9] then minus 5 -> raw in
    // [-14, 4], jitter factor in [0.86, 1.04]. Compute fresh each tick;
    // direction itself stays stable.
    let raw = rng.next_raw_modulo_signed(10) - 5;
    let jitter = SimFixed::from_num(1) + SimFixed::from_num(raw) * SimFixed::from_num(0.01);
    p.prev_delta = [
        p.direction[0] * jitter,
        p.direction[1] * jitter,
        p.direction[2] * jitter,
    ];

    // Lifetime decrement (matches smoke/gas).
    p.lifetime_remaining = p.lifetime_remaining.saturating_sub(1);
    if p.lifetime_remaining <= 0 {
        p.marked_for_deletion = true;
    }

    // Decel — fire decel is per-frame regardless of velocity sign because
    // the velocity-zero gate above already handled the zero case.
    p.velocity = (p.velocity - pt.deacc).max(SIM_ZERO);

    // Damage countdown — only resets when the particle is still in its
    // damaging window (animation_state ≤ final_damage_state). Past that,
    // counter drops below zero and stays there: damage stops permanently.
    p.damage_counter = p.damage_counter.saturating_sub(1);
    if p.damage_counter <= 0 && p.animation_state <= pt.final_damage_state {
        // C6 hooks the damage-to-cell-occupants iteration here.
        p.damage_counter = pt.max_dc as i16;
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
    if p.velocity <= SIM_ZERO {
        return;
    }
    let dx = p.prev_delta[0].to_num::<i32>();
    let dy = p.prev_delta[1].to_num::<i32>();
    let dz = p.prev_delta[2].to_num::<i32>();
    let new_coords = p.coords + IVec3::new(dx, dy, dz);
    if old_ground < new_ground {
        // Cliff death — terrain rises, particle hits ground.
        p.hit_ground = true;
        p.marked_for_deletion = true;
        // Coords still advance — the binary's SetCoords runs after the kill.
    }
    p.previous_coords = p.coords;
    p.coords = new_coords;
}

/// Advance one fire `ParticleSystem` by one tick.
pub(super) fn tick_system(sys: &mut ParticleSystem, sim: &mut Simulation, rules: &RuleSet) {
    let pst = rules.particle_system_type(sys.type_id);
    let cap = pst.particle_cap as usize;
    let tick = u64::from(sim.session.binary_frame);

    // Phase 1 — tick existing particles.
    for p in &mut sys.particles {
        let pt = rules.particle_type(p.type_id);
        let frame_count = super::system_ai::resolve_image_frame_count(rules, pt);
        tick_particle(p, pt, frame_count, sim.particle_rng());
    }

    // Phase 2 — prune dead particles. Fire has no NextParticle chaining
    // (the [FireStream] type has none and no fire chain exists in vanilla).
    sys.particles.retain(|p| !p.marked_for_deletion);

    // Phase 3 — spawn at SpawnFrames cadence via the insert-shuffle helper.
    // Orbital attached-object tracking + the target-moved 3-tick fallback
    // are deferred (see module doc).
    if !sys.done_spawning && pst.spawns {
        let frames = (pst.spawn_frames as u64).max(1);
        if tick % frames == 0 && sys.particles.len() < cap {
            let _ = spawn_particle_with_insert(
                sys,
                sys.coords,
                sys.coords,
                FIRE_INSERT_RANGE,
                rules,
                sim.particle_rng(),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::particle_system_type::ParticleSystemTypeId;
    use crate::sim::particles::ParticleSystem;
    use glam::IVec3;

    fn fake_system(type_id: ParticleSystemTypeId) -> ParticleSystem {
        ParticleSystem {
            stable_id: 0,
            in_logic_vector: false,
            type_id,
            coords: IVec3::ZERO,
            offset: IVec3::ZERO,
            particles: Vec::new(),
            spawn_timer: SimFixed::from_num(1),
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
        let mut p = make_particle(
            ParticleTypeId(0),
            IVec3::ZERO,
            IVec3::ZERO,
            pt,
            sim.particle_rng(),
        );
        p.velocity = SIM_ZERO;
        tick_particle(&mut p, pt, 0, sim.particle_rng());
        assert!(p.marked_for_deletion, "zero-velocity fire dies immediately");
    }

    #[test]
    fn jitter_signed_remainder_dips_below_old_floor() {
        // Old code: next_range_u32(10) - 5 -> raw in [-5, 4] -> jitter >= 0.95.
        // Binary: signed (raw % 10) - 5 -> raw in [-14, 4] -> jitter can reach
        // 0.86, impossible under the old non-negative draw. Drive a fixed-east
        // unit direction so prev_delta[0] == jitter, and target the seed=1 third
        // raw draw (0xDA63B931, negative): -630_998_735 % 10 = -5 -> raw = -10 ->
        // jitter = 1 + (-10)*0.01 = 0.90 (below the old 0.95 floor).
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
        let mut throwaway = SimRng::new(7);
        let mut p = make_particle(
            ParticleTypeId(0),
            IVec3::ZERO,
            IVec3::ZERO,
            pt,
            &mut throwaway,
        );
        p.direction = [SimFixed::from_num(1), SIM_ZERO, SIM_ZERO];
        p.velocity = SimFixed::from_num(100);

        let mut rng = SimRng::new(1);
        rng.next_u32(); // skip draw 1
        rng.next_u32(); // skip draw 2 -> next (3rd) draw is the negative 0xDA63B931
        tick_particle(&mut p, pt, 0, &mut rng);

        // prev_delta[0] == direction[0] * jitter == jitter; must be below the
        // old 0.95 floor (proving the signed range), and near 0.88.
        assert!(
            p.prev_delta[0] < SimFixed::from_num(0.95),
            "signed jitter must reach below the old 0.95 floor, got {}",
            p.prev_delta[0]
        );
        assert!(
            p.prev_delta[0] > SimFixed::from_num(0.80),
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
        let mut p = make_particle(
            ParticleTypeId(0),
            IVec3::ZERO,
            IVec3::ZERO,
            pt,
            sim.particle_rng(),
        );
        // Force animation_state past final_damage_state (default 14).
        p.animation_state = 20;
        // Drive damage_counter to zero — must NOT reset to MaxDC.
        for _ in 0..5 {
            tick_particle(&mut p, pt, 0, sim.particle_rng());
        }
        assert!(
            p.damage_counter <= 0,
            "past FinalDamageState, counter must NOT reset (got {})",
            p.damage_counter
        );
    }

    #[test]
    fn move_fire_marks_dead_when_terrain_rises() {
        // Cliff death: old_ground < new_ground → hit_ground + marked dead.
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
        let mut p = make_particle(
            ParticleTypeId(0),
            IVec3::new(100, 100, 0),
            IVec3::ZERO,
            pt,
            sim.particle_rng(),
        );
        p.prev_delta = [SimFixed::from_num(5), SIM_ZERO, SIM_ZERO];
        // old_ground=0, new_ground=10 → terrain rises.
        move_fire(&mut p, 0, 10);
        assert!(p.hit_ground, "cliff death sets hit_ground");
        assert!(p.marked_for_deletion, "cliff death marks for deletion");
        // Coords advance to the cliff cell — matches binary parity.
        assert_eq!(p.coords, IVec3::new(105, 100, 0));
        assert_eq!(p.previous_coords, IVec3::new(100, 100, 0));
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
        let mut p = make_particle(
            ParticleTypeId(0),
            IVec3::new(100, 100, 0),
            IVec3::ZERO,
            pt,
            sim.particle_rng(),
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
