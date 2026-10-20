//! Original constructor/Logic continuation, bounded to the flat Spark controls
//! in `procedural_drawing_oracle/electric_bolt` and the retained save envelope.

use super::{Particle, ParticleSystem};
use crate::map::resolved_terrain::{ResolvedTerrainGrid, test_flat_cell};
use crate::rules::ini_parser::IniFile;
use crate::rules::particle_system_type::ParticleSystemTypeId;
use crate::rules::particle_type::ParticleTypeId;
use crate::rules::ruleset::RuleSet;
use crate::sim::native_identity::NativeUniqueIdCursor;
use crate::sim::overlay_grid::OverlayGrid;
use crate::sim::rng::SimRng;
use crate::sim::snapshot::GameSnapshot;
use crate::sim::world::Simulation;
use glam::IVec3;
use serde_json::Value;
use std::collections::BTreeMap;

fn corpus() -> Value {
    serde_json::from_str(crate::test_fixture::text(
        "tools/procedural_drawing_oracle/electric_bolt.json",
    ))
    .unwrap()
}

fn rules_for(case: &Value) -> RuleSet {
    let mut sections: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    for layer in case["type_layers"].as_array().unwrap() {
        if let Some(layer_sections) = layer["sections"].as_object() {
            for (section, entries) in layer_sections {
                for (key, value) in entries.as_object().unwrap() {
                    sections
                        .entry(section.clone())
                        .or_default()
                        .insert(key.clone(), value.as_str().unwrap().into());
                }
            }
        }
    }
    if let Some(overrides) = case["input"]["system_type_overrides"].as_object() {
        for (key, value) in overrides {
            sections
                .entry("SparkSys".into())
                .or_default()
                .insert(key.clone(), value.as_str().unwrap().into());
        }
    }
    let mut ini = "[ParticleSystems]\n0=SparkSys\n[Particles]\n0=Spark\n".to_owned();
    for (section, entries) in sections {
        ini.push_str(&format!("[{section}]\n"));
        for (key, value) in entries {
            ini.push_str(&format!("{key}={value}\n"));
        }
    }
    RuleSet::from_ini(&IniFile::from_str(&ini)).unwrap()
}

fn assert_rng(sim: &Simulation, expected: &Value) {
    assert_eq!(
        sim.main_rng.native_state_hex(),
        expected["main"].as_str().unwrap()
    );
    assert_eq!(
        sim.scenario_rng.native_state_hex(),
        expected["scenario"].as_str().unwrap()
    );
    assert_eq!(
        sim.mapgen_rng.native_state_hex(),
        expected["mapgen"].as_str().unwrap()
    );
}

fn assert_state(sim: &Simulation, id: u64, expected: &Value) {
    assert_eq!(
        sim.native_unique_ids.as_ref().unwrap().current_raw(),
        expected["native_id_cursor"].as_i64().unwrap() as u32
    );
    let system = sim.particle_systems().get(id).unwrap();
    let native = &expected["systems"][0];
    assert_eq!(
        system.native_unique_id(),
        native["native_id"].as_i64().unwrap() as i32
    );
    assert_eq!(
        serde_json::json!(system.coords.to_array()),
        native["position"]
    );
    assert_eq!(system.lifetime, native["lifetime"].as_i64().unwrap() as i32);
    assert_eq!(
        system.spark_spawn_frames,
        native["spark_spawn_frames"].as_i64().unwrap() as i32
    );
    assert_eq!(
        system.done_spawning,
        native["time_to_die"].as_bool().unwrap()
    );
    assert_eq!(system.in_logic_vector, native["logic"].as_bool().unwrap());
    assert_eq!(
        system.particles.len(),
        native["particle_count"].as_u64().unwrap() as usize
    );
    let logic_ids: Vec<_> = sim
        .live_object_order_snapshot()
        .into_iter()
        .map(|id| sim.particle_systems().get(id).unwrap().native_unique_id())
        .collect();
    assert_eq!(serde_json::json!(logic_ids), expected["logic_native_ids"]);
    for (particle, native) in system
        .particles
        .iter()
        .zip(expected["particles"].as_array().unwrap())
    {
        assert_eq!(
            particle.native_unique_id(),
            native["native_id"].as_i64().unwrap() as i32
        );
        assert_eq!(
            serde_json::json!(particle.coords.to_array()),
            native["position"]
        );
        assert_eq!(
            particle.lifetime_remaining,
            native["remaining_ec"].as_i64().unwrap() as i16
        );
        assert_eq!(
            particle.marked_for_deletion,
            native["time_to_die"].as_bool().unwrap()
        );
        let spark = particle.spark.as_ref().unwrap();
        assert_eq!(
            serde_json::json!([
                spark.velocity_x.bits(),
                spark.velocity_y.bits(),
                spark.velocity_z.bits()
            ]),
            native["velocity_bits"]
        );
        assert_eq!(serde_json::json!(spark.start_rgb), native["color"]);
        assert_eq!(
            spark.color_index,
            native["color_index"].as_i64().unwrap() as i32
        );
        let color_hex: String = spark
            .color_accumulator
            .bits()
            .to_le_bytes()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        assert_eq!(color_hex, native["color_factor_bits"].as_str().unwrap());
        assert_eq!(
            native["logic"], false,
            "children remain system-owned; only the system is in Logic"
        );
    }
}

#[test]
fn original_spark_birth_and_first_live_visit_preserve_ids_rng_and_particle_state() {
    let corpus = corpus();
    let cases = corpus["spark_cases"]
        .as_array()
        .expect("original Spark controls");
    assert!(!cases.is_empty());
    for case in cases {
        if case["input"]["fail_system_allocation"] == true {
            // Rust allocation failure is not injectable at this constructor.
            continue;
        }
        let rules = rules_for(case);
        let mut sim = Simulation::new();
        let system_entry = case["events"]
            .as_array()
            .unwrap()
            .iter()
            .find(|event| event["pc"] == "0x62dc50")
            .unwrap();
        sim.native_unique_ids = Some(NativeUniqueIdCursor::test_at_current_value(
            system_entry["native_id_before"].as_u64().unwrap() as u32,
        ));
        // EBolt Init's Main draw precedes the system constructor. Adopt that
        // executed boundary; this test covers the actual system/particle path.
        let rng = &case["birth"]["rng_after"];
        sim.main_rng = SimRng::from_native_state_hex_for_test(rng["main"].as_str().unwrap()).into();
        sim.scenario_rng =
            SimRng::from_native_state_hex_for_test(rng["scenario"].as_str().unwrap());
        sim.mapgen_rng = SimRng::from_native_state_hex_for_test(rng["mapgen"].as_str().unwrap());
        let position: [i32; 3] =
            serde_json::from_value(case["endpoint"]["target"].clone()).unwrap();
        let cells = (43..=47)
            .flat_map(|x| (47..=51).map(move |y| test_flat_cell(x, y)))
            .collect();
        sim.resolved_terrain = Some(ResolvedTerrainGrid::from_cells(64, 64, cells));
        sim.overlay_grid = Some(OverlayGrid::new(64, 64));
        let id = sim
            .spawn_particle_system(
                ParticleSystemTypeId(0),
                IVec3::from_array(position),
                None,
                None,
                IVec3::ZERO,
                None,
                &rules,
            )
            .unwrap();
        assert_state(&sim, id, &case["birth"]["state"]);
        assert_rng(&sim, rng);
        sim.object_ai_stage(Some(&rules));
        assert_state(&sim, id, &case["first_logic_visit"]["state"]);
        assert_rng(&sim, &case["first_logic_visit"]["rng_after"]);
        if case["input"]["detail"].as_u64().unwrap_or(2) == 2 {
            // Detail0/1 controls are filtered later by the app's actual
            // Options owner; sim emits the same immutable birth fact.
            let births: Vec<_> = sim
                .combat_light_requests
                .iter()
                .map(|request| {
                    let crate::sim::combat::CombatLightRequest::Spark { coord, base_size } =
                        request
                    else {
                        panic!("Spark visit emitted a different light producer");
                    };
                    serde_json::json!({"position": [coord.x, coord.y, coord.z], "size": base_size})
                })
                .collect();
            let lights: Vec<_> = case["first_logic_visit"]["state"]["lights"].as_array().unwrap()
                .iter().map(|light| serde_json::json!({"position": light["position"], "size": light["size"]})).collect();
            assert_eq!(births, lights);
        }

        // Native630100's own ID and every child GetUniqueID are CRC inputs.
        let system = sim.particle_systems().get(id).unwrap();
        let crc_ids: Vec<_> = case["checksum"]["words"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|word| word["caller"] == "0x410423" || word["caller"] == "0x630135")
            .map(|word| word["value"].as_i64().unwrap() as i32)
            .collect();
        let retained: Vec<_> = std::iter::once(system.native_unique_id())
            .chain(system.particles.iter().map(Particle::native_unique_id))
            .collect();
        assert_eq!(retained, crc_ids);

        // Persistence retains identities and the one allocation cursor; load
        // deliberately reseeds Scenario RNG through the established owner.
        sim.session
            .set_map_name_for_test("spark-native-identities".into());
        let cursor = sim.native_unique_ids.as_ref().unwrap().current_raw();
        let bytes = GameSnapshot::save_validated(&sim, 17, 23, "Spark native identities", 0);
        let mut restored = GameSnapshot::load_validated(&bytes, 17, 23, "spark-native-identities")
            .unwrap()
            .sim;
        restored.restore_after_snapshot_load().unwrap();
        assert_state(&restored, id, &case["first_logic_visit"]["state"]);
        assert_eq!(
            restored.next_native_runtime_id(),
            cursor.wrapping_add(1) as i32
        );
    }
}

#[test]
fn stored_system_and_child_native_ids_each_affect_authoritative_hash() {
    let mut sim = Simulation::new();
    let id = sim.substrate.allocate_stable_id();
    let mut system = ParticleSystem::test_fixture(id, ParticleSystemTypeId(0), IVec3::ZERO);
    system
        .particles
        .push(Particle::test_fixture(ParticleTypeId(0), IVec3::ZERO));
    sim.substrate.particle_systems.insert(system);
    let original = sim.state_hash();
    sim.substrate
        .particle_systems
        .get_mut(id)
        .unwrap()
        .native_unique_id = 17;
    assert_ne!(sim.state_hash(), original);
    sim.substrate
        .particle_systems
        .get_mut(id)
        .unwrap()
        .native_unique_id = 0;
    assert_eq!(sim.state_hash(), original);
    sim.substrate
        .particle_systems
        .get_mut(id)
        .unwrap()
        .particles[0]
        .native_unique_id = 17;
    assert_ne!(sim.state_hash(), original);
}
