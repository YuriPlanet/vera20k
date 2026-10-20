//! Consumed original Gas controls. Scene/type inputs and callback boundaries
//! are declared in virus_gas.meta.json; these are not full native matches.
use super::*;
use crate::map::bridge_facts::{BRIDGE_FLAG_STRUCTURAL, BridgeCellFacts};
use crate::map::resolved_terrain::{ResolvedTerrainGrid, test_flat_cell};
use crate::rules::ruleset::RuleSet;
use crate::sim::native_identity::NativeUniqueIdCursor;
use crate::sim::overlay_grid::OverlayGrid;
use crate::sim::rng::SimRng;
use crate::sim::world::UninitContext;
use serde_json::Value;

fn corpus() -> &'static Value {
    let data = crate::test_fixture::json("tools/spatial_oracle/virus_gas.json");
    assert_eq!(
        data["native_sha256"],
        "1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c"
    );
    data
}
fn rules(input: &Value) -> RuleSet {
    super::native_fixture::rules_from_layers(
        &corpus()["type_layers"],
        input,
        "[Particles]\n0=VirusCloud1\n1=VirusCloudD1\n[ParticleSystems]\n0=GasCloudSys\n",
    )
}
fn coords(value: &Value) -> IVec3 {
    IVec3::from_array(serde_json::from_value(value.clone()).unwrap())
}
fn bits(value: &Value) -> NativeF32Bits {
    NativeF32Bits::from_bits(u32::from_str_radix(value.as_str().unwrap(), 16).unwrap())
}
fn setup(rules: &RuleSet, input: &Value, location: IVec3) -> (Simulation, u64) {
    let mut sim = Simulation::with_seed(1);
    sim.native_unique_ids = Some(NativeUniqueIdCursor::test_at_current_value(1000));
    let cells = (0..64)
        .flat_map(|y| {
            (0..64).map(move |x| {
                let mut cell = test_flat_cell(x, y);
                if input["bridge"] == true && x == 44 && y == 48 {
                    cell.bridge_facts = BridgeCellFacts {
                        raw_flags: BRIDGE_FLAG_STRUCTURAL,
                        ..Default::default()
                    };
                }
                cell
            })
        })
        .collect();
    sim.resolved_terrain = Some(ResolvedTerrainGrid::from_cells(64, 64, cells));
    sim.overlay_grid = Some(OverlayGrid::new(64, 64));
    let system = sim
        .spawn_particle_system(
            rules.ps_type_id_by_name("GasCloudSys").unwrap(),
            location,
            None,
            None,
            IVec3::ZERO,
            None,
            rules,
        )
        .unwrap();
    // The native fixture explicitly installs this system in DAT_A8ED78.
    sim.substrate.particle_systems.gas_cloud_system = Some(system);
    (sim, system)
}
fn assert_particle(p: &Particle, expected: &Value, name: &str) {
    assert_eq!(
        p.native_unique_id(),
        expected["native_id"].as_i64().unwrap() as i32,
        "{name}: ID"
    );
    assert_eq!(p.coords, coords(&expected["coords"]), "{name}: coordinate");
    assert_eq!(
        p.motion.map(NativeF32Bits::bits),
        expected["direction_bits"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| bits(x).bits())
            .collect::<Vec<_>>()
            .as_slice(),
        "{name}: motion words"
    );
    assert_eq!(
        p.velocity,
        bits(&expected["velocity_bits"]),
        "{name}: velocity"
    );
    assert_eq!(
        [p.drift_x, p.drift_y, p.drift_z],
        serde_json::from_value::<[i32; 3]>(expected["drift"].clone()).unwrap(),
        "{name}: drift"
    );
    assert_eq!(
        p.lifetime_remaining as u16,
        expected["remaining_ec"].as_u64().unwrap() as u16,
        "{name}: EC"
    );
    assert_eq!(
        p.damage_counter,
        expected["damage_counter"].as_i64().unwrap() as i16,
        "{name}: DC"
    );
    assert_eq!(
        p.animation_state,
        expected["state"].as_u64().unwrap() as u8,
        "{name}: animation"
    );
    assert_eq!(
        p.translucency,
        expected["translucency"].as_u64().unwrap() as u8,
        "{name}: translucency"
    );
    assert_eq!(
        p.marked_for_deletion,
        expected["marked"].as_bool().unwrap(),
        "{name}: terminal"
    );
}
fn assert_system(sim: &Simulation, id: u64, expected: &Value, name: &str) {
    let sys = sim.particle_systems().get(id).unwrap();
    assert_eq!(
        sys.native_unique_id(),
        expected["native_id"].as_i64().unwrap() as i32,
        "{name}"
    );
    assert_eq!(
        sys.lifetime,
        expected["lifetime"].as_i64().unwrap() as i32,
        "{name}"
    );
    assert_eq!(sys.coords, coords(&expected["coords"]), "{name}");
    assert_eq!(
        sys.done_spawning,
        expected["done"].as_bool().unwrap(),
        "{name}"
    );
    assert_eq!(sys.alive(), expected["alive"].as_bool().unwrap(), "{name}");
    assert_eq!(
        sys.in_limbo(),
        expected["limbo"].as_bool().unwrap(),
        "{name}"
    );
    let particles = expected["particles"].as_array().unwrap();
    assert_eq!(sys.particles.len(), particles.len(), "{name}: children");
    for (particle, expected) in sys.particles.iter().zip(particles) {
        assert_eq!(
            sim_type(sim, particle),
            expected["type"].as_str().unwrap(),
            "{name}"
        );
        assert_particle(particle, expected, name);
    }
}
fn sim_type(_sim: &Simulation, p: &Particle) -> &'static str {
    match p.type_id.0 {
        0 => "VirusCloud1",
        1 => "VirusCloudD1",
        _ => panic!("unexpected fixture type"),
    }
}

#[test]
fn gas_system_motion_lifetime_replacement_and_rng_match_original_histories() {
    for case in corpus()["controls"].as_array().unwrap() {
        let name = case["input"]["name"].as_str().unwrap();
        let rules = rules(&case["input"]);
        let (mut sim, system) = setup(&rules, &case["input"], coords(&case["before"]["coords"]));
        for prior in case["before"]["particles"].as_array().unwrap() {
            let kind = rules
                .p_type_id_by_name(prior["type"].as_str().unwrap())
                .unwrap();
            let mut p = Particle::new(
                kind,
                coords(&prior["coords"]),
                IVec3::ZERO,
                rules.particle_type(kind),
                &mut sim,
            );
            p.motion = std::array::from_fn(|i| bits(&prior["direction_bits"][i]));
            p.velocity = bits(&prior["velocity_bits"]);
            p.lifetime_remaining = prior["remaining_ec"].as_u64().unwrap() as i16;
            p.damage_counter = prior["damage_counter"].as_i64().unwrap() as i16;
            p.animation_state = prior["state"].as_u64().unwrap() as u8;
            p.translucency = prior["translucency"].as_u64().unwrap() as u8;
            [p.drift_x, p.drift_y, p.drift_z] =
                serde_json::from_value(prior["drift"].clone()).unwrap();
            assert_particle(&p, prior, name);
            sim.substrate
                .particle_systems
                .get_mut(system)
                .unwrap()
                .particles
                .push(p);
        }
        // The original case supplies RNG AFTER construction and explicit
        // prior EC/DC/drift writes. Only subsequent original AI is compared.
        sim.scenario_rng =
            SimRng::from_native_state_hex_for_test(case["scenario_before"].as_str().unwrap());
        let other_rng = (sim.main_rng.logical_state(), sim.mapgen_rng.logical_state());
        for visit in case["visits"].as_array().unwrap() {
            sim.session
                .set_binary_frame_for_test(visit["frame"].as_u64().unwrap() as u32);
            assert!(super::system_ai::tick_particle_system(
                &mut sim,
                &rules,
                system,
                UninitContext::with_rules(&rules)
            ));
            assert_system(&sim, system, &visit["state"], name);
        }
        assert_eq!(
            sim.scenario_rng.native_state_hex(),
            case["scenario_after"],
            "{name}: full Scenario state"
        );
        assert_eq!(
            (sim.main_rng.logical_state(), sim.mapgen_rng.logical_state()),
            other_rng,
            "{name}: other streams"
        );
    }
}

#[test]
fn one_and_three_explicit_gas_births_match_original_constructors_and_rng() {
    let rules = rules(&Value::Null);
    for case in corpus()["constructors"].as_array().unwrap() {
        let (mut sim, system) = setup(&rules, &Value::Null, coords(&case["state"]["coords"]));
        sim.scenario_rng =
            SimRng::from_native_state_hex_for_test(case["scenario_before"].as_str().unwrap());
        let kind = rules.p_type_id_by_name("VirusCloud1").unwrap();
        for _ in 0..case["count"].as_u64().unwrap() {
            sim.spawn_gas_particle(kind, IVec3::new(11392, 12416, 0), &rules)
                .unwrap();
        }
        assert_system(&sim, system, &case["state"], "explicit births");
        assert_eq!(sim.scenario_rng.native_state_hex(), case["scenario_after"]);
        assert_eq!(
            sim.native_unique_ids.as_ref().unwrap().current_raw() as u32,
            case["cursor_after"].as_u64().unwrap() as u32
        );
    }
}
