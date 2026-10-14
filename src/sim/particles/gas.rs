//! Gas `BehavesLike` system + particle AI.
//!
//! Per-tick gas logic for both the system and the individual particles
//! (state and damage countdown; the dispatcher owns lifetime).
//! `ParticleSystemClass::AI_Gas @
//! 0x0062E6D0` has no periodic spawn arm and no `Slowdown`/`SpawnCutoff`
//! accumulator — those belong to `AI_Smoke @ 0x0062ED40` alone. Gas differs
//! from smoke in two further ways:
//!   - `NextParticle` chains spawn a SINGLE child at the parent's position
//!     (offset by `NextParticleOffset`), not two symmetric children.
//!   - The child copies the parent's velocity AND its drift_x/y/z fields,
//!     so a moving gas plume keeps drifting after a chain step.
//!
//! `move_gas` uses the `GAS_WIND_*` tables (which differ from smoke at SE).
//! Wind apply is gated by odd-frame parity and `tick % (10 / wind_effect)`.
//!
//! ## Deferred (tracked for follow-up tasks)
//! - Damage application to cell occupants when the damage counter hits zero.
//!   Needs `OccupancyGrid` cell-iteration helpers + a Position (cell coords)
//!   ↔ IVec3 leptons converter; lands with Task C6. The "no friend-or-foe
//!   filter" parity rule lives there. C3 only does the countdown bookkeeping.
//! - Bridge collision in `move_gas`.
//! - Wiring `move_gas` into the per-tick path; General's WindDirection reader
//!   is available, but this separate Gas mechanism remains incomplete.
//! - Per-particle 1-in-8 even-frame random drift.
//! - Gravity (Z velocity = -2.0 - RulesClass.Gravity).

use super::wind::{GAS_WIND_DX, GAS_WIND_DY};
use super::{Particle, ParticleSystem};
use crate::rules::particle_type::{ParticleType, ParticleTypeId};
use crate::rules::ruleset::RuleSet;
use crate::sim::world::Simulation;
use crate::util::native_x87::NativeF32Bits;
use glam::IVec3;

/// Advance one gas `ParticleSystem` by one tick.
pub(super) fn tick_system(sys: &mut ParticleSystem, sim: &mut Simulation, rules: &RuleSet) {
    let pst = rules.particle_system_type(sys.type_id);
    let cap = pst.particle_cap as usize;

    // `ParticleSystemClass::AI_Gas @ 0x0062E6D0` walks its particle array
    // BACKWARDS in both loops (`for (i = count - 1; i >= 0; i--)`), unlike
    // `AI_Smoke`, whose first loop runs forward. Both are kept native-order
    // here.
    //
    // RESIDUAL (GSI-05.13) — the order alone does not close the cap behaviour.
    // Native appends each chain child DURING the walk and tests the cap with
    // the dying parents still counted, because the particle's own destroy
    // vtable fires only after the append; VERA prunes first and then appends,
    // so it has more headroom and drops a different set once a system is at
    // capacity. Trigger: a Gas system at `ParticleCap` with several parents
    // dying in one tick. Player effect: none today — nothing spawns a Gas
    // system. Frequency: zero until a producer lands. Downstream risk: the
    // dropped set is state, so it must be settled with that producer rather
    // than after it.
    //
    // Phase 1 — tick existing particles.
    for p in sys.particles.iter_mut().rev() {
        super::system_ai::tick_particle(p, sim, rules);
    }

    // Phase 2 — collect NextParticle children for any dying particle, then prune.
    // Single-child finding: each dying parent spawns ONE child at
    // `parent.coords + pt.next_particle_offset`, copying velocity + drift.
    let mut child_specs: Vec<ChildSpec> = Vec::new();
    for p in sys.particles.iter().rev() {
        if !p.marked_for_deletion {
            continue;
        }
        let pt = rules.particle_type(p.type_id);
        let Some(next_id) = pt.next_particle else {
            continue;
        };
        child_specs.push(ChildSpec {
            next_id,
            coords: p.coords + pt.next_particle_offset,
            velocity: p.velocity,
            translucency: p.translucency,
            drift_x: p.drift_x,
            drift_y: p.drift_y,
            drift_z: p.drift_z,
        });
    }
    for p in &sys.particles {
        if p.marked_for_deletion {
            sim.conceal_particle(p.stable_id());
        }
    }
    sys.particles.retain(|p| !p.marked_for_deletion);
    for spec in child_specs {
        if sys.particles.len() >= cap {
            break;
        }
        let pt = rules.particle_type(spec.next_id);
        sys.particles.push(make_child(spec, pt, sim));
    }

    // `AI_Gas` ends here. There is no Phase 3 and no Phase 4: the periodic
    // spawn arm and the `Slowdown`/`SpawnCutoff` accumulator live only in
    // `ParticleSystemClass::AI_Smoke @ 0x0062ED40`, and both were imported into
    // this function by mistake. Gas systems therefore never retire themselves —
    // `done_spawning` is written for them by exactly one site, the `Lifetime=`
    // countdown in `system_ai.rs`, mirroring `ParticleSystemClass::AI @
    // 0x0062FD60`'s `sys[0xEC]` decrement. With `Lifetime=` absent on both stock
    // Gas systems that countdown never fires, so the system persists, which is
    // what native does.
}

/// GasAI62BD50 scalar/cadence suffix. Its motion/damage application prefix
/// remains a separate incomplete Gas mechanism; no native scalar decel exists.
pub(super) fn tick_particle(p: &mut Particle, pt: &ParticleType) {
    p.damage_counter = p.damage_counter.wrapping_sub(1);
    if p.damage_counter == 0 && pt.damage != 0 {
        //62C229: reload precedes occupancy damage, which remains unported.
        p.damage_counter = pt.max_dc as i16;
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

/// Apply one tick of gas-table wind drift to `p.coords`. Caller supplies the
/// global wind direction (FacingType 0..7) and the current sim tick;
/// `wind_effect` from the particle type acts as a frequency divider, not a
/// magnitude scale (this is the smoke/gas asymmetry).
///
/// Bridge collision is deferred — see module-level notes.
// Live dispatch still lacks the rules-owned wind direction and bridge query.
#[allow(dead_code)]
pub(super) fn move_gas(p: &mut Particle, pt: &ParticleType, tick: u64) {
    move_gas_with_wind(p, pt, tick, gas_wind_dir());
}

/// Internal helper exposed for testing — lets a test pin a specific wind
/// direction without touching global rules state.
#[allow(dead_code)]
pub(super) fn move_gas_with_wind(p: &mut Particle, pt: &ParticleType, tick: u64, wind_dir: u8) {
    // Gas movement only progresses on odd frames.
    if tick & 1 == 0 {
        return;
    }
    let we = pt.wind_effect as u64;
    if let Some(period) = 10u64.checked_div(we) {
        let period = period.max(1);
        if tick.is_multiple_of(period) {
            let idx = (wind_dir as usize).min(7);
            p.coords.x += GAS_WIND_DX[idx];
            p.coords.y += GAS_WIND_DY[idx];
        }
    }
    p.coords.x += p.drift_x;
    p.coords.y += p.drift_y;
    p.coords.z += p.drift_z;
}

/// Default gas wind direction. Real value comes from `[General] WindDirection=`
/// once that's parsed; until then everything stays at index 0 (north).
#[allow(dead_code)]
fn gas_wind_dir() -> u8 {
    0
}

struct ChildSpec {
    next_id: ParticleTypeId,
    coords: IVec3,
    velocity: NativeF32Bits,
    translucency: u8,
    drift_x: i32,
    drift_y: i32,
    drift_z: i32,
}

fn make_child(spec: ChildSpec, pt: &ParticleType, sim: &mut Simulation) -> Particle {
    let mut p = Particle::new(spec.next_id, spec.coords, IVec3::ZERO, pt, sim);
    p.velocity = spec.velocity;
    p.translucency = spec.translucency;
    p.drift_x = spec.drift_x;
    p.drift_y = spec.drift_y;
    p.drift_z = spec.drift_z;
    p
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::particle_system_type::ParticleSystemTypeId;
    use crate::sim::particles::ParticleSystem;
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
    fn next_particle_spawns_one_child_with_velocity_copy() {
        // Gas chain GasA → GasB. Parent dies this tick; system must spawn
        // exactly ONE child carrying parent's velocity + drift.
        let rules = parse(
            "[Particles]\n\
             1=GasA\n\
             2=GasB\n\
             [GasA]\n\
             BehavesLike=Gas\n\
             MaxEC=1\n\
             NextParticle=GasB\n\
             [GasB]\n\
             BehavesLike=Gas\n\
             MaxEC=10\n\
             [ParticleSystems]\n\
             1=Sys\n\
             [Sys]\n\
             BehavesLike=Gas\n\
             HoldsWhat=GasA\n\
             ParticleCap=20\n",
        );
        let mut sim = Simulation::new();
        let mut sys = fake_system(ParticleSystemTypeId(0));
        let pt_a = rules.particle_type(ParticleTypeId(0));
        let mut parent = Particle::new(
            ParticleTypeId(0),
            IVec3::new(1000, 2000, 0),
            IVec3::ZERO,
            pt_a,
            &mut sim,
        );
        parent.lifetime_remaining = 1;
        parent.velocity = NativeF32Bits::from_bits(0x4040_0000);
        parent.drift_x = 7;
        parent.drift_y = -4;
        parent.drift_z = 11;
        sys.particles.push(parent);

        tick_system(&mut sys, &mut sim, &rules);

        // Single-child chain: exactly one child, original parent removed.
        assert_eq!(sys.particles.len(), 1, "single-child finding");
        let child = &sys.particles[0];
        assert_eq!(child.type_id, ParticleTypeId(1));
        // Parent's velocity carries to the child.
        assert_eq!(child.velocity, NativeF32Bits::from_bits(0x4040_0000));
        // Parent's drift carries too — a moving plume keeps drifting.
        assert_eq!(child.drift_x, 7);
        assert_eq!(child.drift_y, -4);
        assert_eq!(child.drift_z, 11);
        // Child spawns at parent's position (NextParticleOffset defaults to 0,0,0).
        assert_eq!(child.coords, IVec3::new(1000, 2000, 0));
    }

    #[test]
    fn damage_countdown_resets_from_max_dc() {
        // MaxDC=5: ticking exactly 5 frames should land the counter back at 5
        // (decrement each tick, hit zero on tick 5, reset to MaxDC).
        let rules = parse(
            "[Particles]\n\
             1=Gas\n\
             [Gas]\n\
             BehavesLike=Gas\n\
             MaxDC=5\n\
             MaxEC=1000\n\
             Damage=10\n",
        );
        let pt = rules.particle_type(ParticleTypeId(0));
        let mut sim = Simulation::new();
        let mut p = Particle::new(ParticleTypeId(0), IVec3::ZERO, IVec3::ZERO, pt, &mut sim);
        // The shared constructor initializes damage_counter = pt.max_dc.
        assert_eq!(p.damage_counter, 5);
        for _ in 0..5 {
            tick_particle(&mut p, pt);
        }
        assert_eq!(
            p.damage_counter, 5,
            "after exactly MaxDC ticks, counter resets to MaxDC"
        );
    }

    #[test]
    fn move_gas_uses_gas_wind_table_se_dx_is_one() {
        // SE wind direction (idx 3): gas DX=1, smoke DX=2. wind_effect=2 gives
        // period = 10/2 = 5; tick=5 is odd AND satisfies tick % 5 == 0, so the
        // wind apply triggers and we read GAS_WIND_DX[3] (== 1), not the smoke
        // value.
        let rules = parse(
            "[Particles]\n\
             1=Gas\n\
             [Gas]\n\
             BehavesLike=Gas\n\
             WindEffect=2\n\
             MaxEC=10\n",
        );
        let pt = rules.particle_type(ParticleTypeId(0));
        let mut sim = Simulation::new();
        let mut p = Particle::new(ParticleTypeId(0), IVec3::ZERO, IVec3::ZERO, pt, &mut sim);
        move_gas_with_wind(&mut p, pt, 5, 3);
        assert_eq!(p.coords.x, 1, "GAS_WIND_DX[3] == 1");
        assert_eq!(p.coords.y, 2, "GAS_WIND_DY[3] == 2");
    }

    #[test]
    fn gas_system_never_spawns_periodically_even_with_spawns_authored() {
        // `ParticleSystemClass::AI_Gas @ 0x0062E6D0` has no periodic spawn arm:
        // `Spawns=`/`SpawnFrames=` are read by `AI_Smoke @ 0x0062ED40` only. A
        // Gas system therefore emits nothing on its own, however it is authored.
        let rules = parse(
            "[Particles]
             1=Gas
             [Gas]
             BehavesLike=Gas
             MaxEC=1000
             [ParticleSystems]
             1=Sys
             [Sys]
             BehavesLike=Gas
             HoldsWhat=Gas
             ParticleCap=5
             SpawnFrames=1
             Spawns=yes
",
        );
        let mut sim = Simulation::new();
        let mut sys = fake_system(ParticleSystemTypeId(0));
        let rng_before = sim.particle_rng().state();
        for _ in 0..50 {
            tick_system(&mut sys, &mut sim, &rules);
            sim.session.binary_frame = sim.session.binary_frame.wrapping_add(1);
        }
        assert!(
            sys.particles.is_empty(),
            "AI_Gas spawned {} particles; native spawns none",
            sys.particles.len()
        );
        // The two smoke-imported spawn-offset draws are gone with the arm.
        assert_eq!(
            sim.particle_rng().state(),
            rng_before,
            "an idle Gas system must consume no particle-RNG draws"
        );
    }

    #[test]
    fn gas_system_never_sets_done_spawning_from_the_spawn_cutoff() {
        // `AI_Gas` never reads `Slowdown` (`psType+0x2A0`) or `SpawnCutoff`
        // (`psType+0x2AC`) and never writes the system's `+0xF8` retirement
        // byte. With the stock defaults (`spawn_timer` 1, `spawn_cutoff` 0) the
        // imported accumulator tripped on the very first tick.
        let rules = parse(
            "[Particles]
             1=Gas
             [Gas]
             BehavesLike=Gas
             MaxEC=1000
             [ParticleSystems]
             1=Sys
             [Sys]
             BehavesLike=Gas
             HoldsWhat=Gas
             ParticleCap=20
",
        );
        let mut sim = Simulation::new();
        let mut sys = fake_system(ParticleSystemTypeId(0));
        for _ in 0..10 {
            tick_system(&mut sys, &mut sim, &rules);
            sim.session.binary_frame = sim.session.binary_frame.wrapping_add(1);
        }
        assert!(
            !sys.done_spawning,
            "only the Lifetime countdown retires a Gas system"
        );
        assert_eq!(
            sys.spawn_timer,
            NativeF32Bits::ONE,
            "no accumulator advances the Gas spawn timer"
        );
    }
}
