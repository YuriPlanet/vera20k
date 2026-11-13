//! Ares ParticleSystemExt through the production system AI. The expected
//! Scenario draws replay the order the DLL takes them in (UpdateSpark
//! `10028370`, UpdateRailgun `10026E70`, UpdateSmoke `10027950` and its
//! movement `100280D0`, the colour step `10026D30`).

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::sim::rng::SimRng;
use crate::sim::world::UninitContext;

fn rules(ini: &str) -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(ini)).expect("particle rules parse")
}

fn spawn(sim: &mut Simulation, rules: &RuleSet, name: &str, at: IVec3, target: IVec3) -> u64 {
    sim.spawn_particle_system(
        rules.ps_type_id_by_name(name).unwrap(),
        at,
        None,
        None,
        target,
        None,
        rules,
    )
    .expect("system spawns")
}

fn tick(sim: &mut Simulation, rules: &RuleSet, id: u64) {
    assert!(super::super::system_ai::tick_particle_system(
        sim,
        rules,
        id,
        UninitContext::with_rules(rules),
    ));
}

fn ext(sim: &Simulation, id: u64) -> &AresParticleSystemExt {
    sim.particle_systems()
        .get(id)
        .unwrap()
        .ares()
        .expect("Ares simulates the system")
}

fn unit(rng: &mut SimRng) -> i32 {
    rng.next_range_i32_inclusive(1, i32::MAX)
}

/// A one-frame spark burst: the radius walk draw first, no percentage draw on
/// the last spawn frame, `ParticleCap/2 + 0..ParticleCap/2` sparks with three
/// direction draws and a lifetime draw each, then one colour draw per spark.
/// No ParticleClass is built, and the done system retires only once its last
/// record dies.
#[test]
fn spark_burst_takes_ares_draws_and_retires_after_its_records() {
    let rules = rules(
        "[ParticleSystems]\n0=Sparks\n[Sparks]\nBehavesLike=Spark\nHoldsWhat=Spark\n\
         ParticleCap=6\nSparkSpawnFrames=1\n\
         [Particles]\n0=Spark\n[Spark]\nBehavesLike=Spark\nMaxEC=10\nXVelocity=10\n\
         YVelocity=10\nMinZVelocity=4\nZVelocityRange=5\n\
         ColorList=(255,255,255),(128,128,64)\nColorSpeed=0.1\n",
    );
    let mut sim = Simulation::with_seed(41);
    let id = spawn(
        &mut sim,
        &rules,
        "Sparks",
        IVec3::new(2000, 2000, 400),
        IVec3::ZERO,
    );
    assert_eq!(ext(&sim, id).mode(), AresSystemMode::Spark);
    let mut expected = sim.clone_scenario_rng();

    tick(&mut sim, &rules, id);

    let jitter = f64::from(unit(&mut expected)) / 2_147_483_648.0;
    let count = expected.next_range_i32_inclusive(0, 3) + 3;
    let mut lifetimes = Vec::new();
    for _ in 0..count {
        expected.next_range_i32_inclusive(0, 10);
        expected.next_range_i32_inclusive(0, 10);
        expected.next_range_i32_inclusive(0, 5);
        lifetimes.push(expected.next_range_i32_inclusive(0, 10) + 10 - 1);
    }
    for _ in 0..count {
        unit(&mut expected);
    }
    assert_eq!(sim.scenario_rng.logical_state(), expected.logical_state());
    let system = sim.particle_systems().get(id).unwrap();
    assert!(system.particles().is_empty(), "no ParticleClass (0x62E380)");
    assert!(system.done_spawning());
    let radius = if jitter < 0.3 {
        0x1D - 3
    } else if jitter < 0.6 {
        0x1D + 3
    } else {
        0x1D
    };
    assert_eq!(i32::from(system.facing()), radius);
    let sparks = ext(&sim, id).particles();
    assert_eq!(
        sparks
            .iter()
            .map(|spark| spark.lifetime())
            .collect::<Vec<_>>(),
        lifetimes
    );
    assert!(sparks.iter().all(|spark| spark.color() == [255, 255, 255]));

    let longest = *lifetimes.iter().max().unwrap();
    for _ in 1..longest {
        tick(&mut sim, &rules, id);
        assert!(!sim.substrate.pending_delete.contains_object(&id));
    }
    tick(&mut sim, &rules, id);
    assert!(ext(&sim, id).particles().is_empty());
    assert!(sim.substrate.pending_delete.contains_object(&id));
}

/// A railgun lays `(int)(ParticlesPerCoord * distance)` records (here
/// 1/32 x 640 = 20, both exact) on its first
/// frame, each taking seven jitter draws and a 0..9 lifetime draw; then every
/// record takes a speed draw and a colour draw.
#[test]
fn railgun_lays_ares_records_with_their_draws() {
    let rules = rules(
        "[ParticleSystems]\n0=Rail\n[Rail]\nBehavesLike=Railgun\nHoldsWhat=RailPart\n\
         ParticlesPerCoord=0.03125\nSpiralRadius=10\nPositionPerturbationCoefficient=2\n\
         MovementPerturbationCoefficient=1\nVelocityPerturbationCoefficient=1\n\
         [Particles]\n0=RailPart\n[RailPart]\nBehavesLike=Railgun\nMaxEC=10\nVelocity=2\n",
    );
    let mut sim = Simulation::with_seed(43);
    let id = spawn(&mut sim, &rules, "Rail", IVec3::new(640, 0, 0), IVec3::ZERO);
    let mut expected = sim.clone_scenario_rng();

    tick(&mut sim, &rules, id);

    let mut lifetimes = Vec::new();
    for _ in 0..20 {
        for _ in 0..7 {
            unit(&mut expected);
        }
        lifetimes.push(expected.next_range_i32_inclusive(0, 9) + 10 - 1);
    }
    for _ in 0..40 {
        unit(&mut expected);
    }
    assert_eq!(sim.scenario_rng.logical_state(), expected.logical_state());
    let system = sim.particle_systems().get(id).unwrap();
    assert!(system.particles().is_empty());
    assert!(system.done_spawning());
    let records = ext(&sim, id).particles();
    assert_eq!(
        records
            .iter()
            .map(|record| record.lifetime())
            .collect::<Vec<_>>(),
        lifetimes
    );
    // The first record starts at the target end of the rail.
    assert!(records[0].coords().x.abs() < 20);
}

/// Smoke spawns one record near the system each period (two offset draws and
/// a lifetime draw), rising at no less than 2; on odd frames each record
/// first takes the 1-in-4 drift draws.
#[test]
fn smoke_spawns_and_drifts_with_ares_draws() {
    let rules = rules(
        "[ParticleSystems]\n0=Smokes\n[Smokes]\nBehavesLike=Smoke\nHoldsWhat=Puff\n\
         SpawnFrames=1\nSpawnRadius=8\nSpawnCutoff=100\n\
         [Particles]\n0=Puff\n[Puff]\nBehavesLike=Smoke\nMaxEC=40\nVelocity=1.0\n\
         StartStateAI=2\nEndStateAI=9\nTranslucency=25\n",
    );
    let mut sim = Simulation::with_seed(47);
    let at = IVec3::new(1000, 2000, 300);
    let id = spawn(&mut sim, &rules, "Smokes", at, IVec3::ZERO);
    assert_eq!(ext(&sim, id).mode(), AresSystemMode::Smoke);
    let unique_id = sim.particle_systems().get(id).unwrap().native_unique_id();
    let mut expected = sim.clone_scenario_rng();

    sim.session.set_binary_frame_for_test(2);
    tick(&mut sim, &rules, id);

    let x = at.x + expected.next_range_i32_inclusive(-9, 9);
    let y = at.y + expected.next_range_i32_inclusive(-9, 9);
    let lifetime = expected.next_range_i32_inclusive(0, 40) + 40;
    assert_eq!(sim.scenario_rng.logical_state(), expected.logical_state());
    let smoke = &ext(&sim, id).smoke()[0];
    assert_eq!(smoke.location(), IVec3::new(x, y, at.z + 10));
    assert_eq!(smoke.rise_speed(), TWO_F32);
    assert_eq!(smoke.phase(), unique_id + 1);
    assert_eq!(smoke.state(), 2);
    assert_eq!(smoke.lifetime(), lifetime);
    // SpawnFrames 1.0 is above the default SpawnTranslucencyCutoff 0.
    assert_eq!(smoke.translucency(), 50);

    sim.session.set_binary_frame_for_test(3);
    tick(&mut sim, &rules, id);

    let mut drift = [0, 0];
    if expected.next_range_i32_inclusive(0, 3) == 0 {
        let delta = expected.next_range_i32_inclusive(-1, 1);
        let axis = usize::from(expected.next_range_i32_inclusive(0, 1) == 0);
        drift[axis] += delta;
    }
    expected.next_range_i32_inclusive(-9, 9);
    expected.next_range_i32_inclusive(-9, 9);
    expected.next_range_i32_inclusive(0, 40);
    assert_eq!(sim.scenario_rng.logical_state(), expected.logical_state());
    let records = ext(&sim, id).smoke();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].drift(), drift);
    assert_eq!(
        records[0].location(),
        IVec3::new(x + drift[0], y + drift[1], at.z + 12)
    );
    assert_eq!(records[0].lifetime(), lifetime - 1);
    assert_eq!(records[1].phase(), unique_id + 2);
}
