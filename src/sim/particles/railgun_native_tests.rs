//! Native comparison: `tools/procedural_drawing_oracle/railgun.json` runs the
//! original ParticleSystem constructor (`0x0062DC50`) with FireAt's railgun
//! arguments, then live Logic through AI_Railgun (`0x0062F230`), its laser
//! (`0x0054FE60`) and the Railgun particle AI (`0x0062C3A0`) to expiry.

use crate::sim::native_identity::NativeUniqueIdCursor;
use crate::sim::rng::SimRng;
use crate::sim::world::{LifecycleOutput, Simulation};
use glam::IVec3;
use serde_json::{Value, json};

fn int(value: &Value) -> i32 {
    value.as_i64().unwrap() as i32
}

fn xyz(value: &Value) -> IVec3 {
    IVec3::new(int(&value[0]), int(&value[1]), int(&value[2]))
}

fn particle_state(particle: &super::Particle) -> Value {
    json!({
        "native_id": particle.native_unique_id(),
        "location": particle.coords().to_array(),
        "direction_bits": particle.direction().map(|axis| axis.bits()),
        "speed_bits": particle.velocity().bits(),
        "position_bits": particle.float_position().map(|axis| axis.bits()),
        "color": particle.start_rgb(),
        "color_index": particle.color_index(),
        "color_accumulator_bits": particle.color_accumulator().bits(),
        "remaining": particle.lifetime_remaining(),
    })
}

#[test]
fn railgun_system_matches_native_lay_laser_and_particles_to_expiry() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/procedural_drawing_oracle/railgun.json",
    ))
    .unwrap();
    let type_id = rules.ps_type_id_by_name("SmallRailgunSys").unwrap();
    for case in corpus["cases"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let mut sim = Simulation::new();
        sim.session.set_binary_frame_for_test(36);
        sim.session.set_map_width_for_test(64);
        sim.session.set_map_height_for_test(64);
        sim.install_resolved_terrain_for_new_map(crate::map::resolved_terrain::test_grid(
            64,
            64,
            crate::map::resolved_terrain::test_clear_cell,
        ));
        sim.intern_rule_type_ids(rules);
        sim.resolve_type_handles(rules);
        sim.set_logic_order_for_test(Vec::new());
        sim.native_unique_ids = Some(NativeUniqueIdCursor::test_at_current_value(1000));
        sim.scenario_rng =
            SimRng::from_native_state_hex_for_test(case["initial_scenario_rng"].as_str().unwrap());
        let target = xyz(&case["target"]);
        let id = sim
            .spawn_particle_system(
                type_id,
                xyz(&case["source"]),
                None,
                None,
                target,
                None,
                rules,
            )
            .unwrap();

        let mut lasers = Vec::new();
        for visit in 1..=int(&corpus["visits"]) {
            sim.session.set_binary_frame_for_test(36 + visit as u32);
            sim.object_ai_stage(Some(rules));
            lasers.extend(sim.lifecycle_outputs.as_slice().iter().filter_map(
                |output| match output {
                    LifecycleOutput::LaserCreated(laser) => Some(*laser),
                    _ => None,
                },
            ));
            sim.lifecycle_outputs.clear_for_test();
            let system = sim
                .particle_systems()
                .get(id)
                .filter(|system| system.in_logic_vector());
            assert_eq!(
                system.map_or(-1, |system| system.particles().len() as i64),
                case["live_counts"][visit as usize - 1].as_i64().unwrap(),
                "{name}: live particles after visit {visit}"
            );
            if let Some(expected) = case["particles_after_visit"].get(visit.to_string()) {
                let actual: Vec<Value> = system
                    .map(|system| system.particles().iter().map(particle_state).collect())
                    .unwrap_or_default();
                assert_eq!(
                    Value::Array(actual),
                    *expected,
                    "{name}: particles after visit {visit}"
                );
            }
        }
        assert_eq!(
            sim.scenario_rng.native_state_hex(),
            case["final_scenario_rng"].as_str().unwrap(),
            "{name}: Scenario draws"
        );

        // Raw 54FE60 arguments: source, target, zAdjust, width, inner, outer,
        // spread (low three bytes each), duration, blinks, fades, start, end.
        let [native] = case["laser_args"].as_array().unwrap().as_slice() else {
            panic!("{name}: one native laser");
        };
        let word = |index: usize| native[index].as_u64().unwrap() as u32;
        let rgb = |index: usize| {
            let [r, g, b, _] = word(index).to_le_bytes();
            [r, g, b]
        };
        let [laser] = lasers.as_slice() else {
            panic!("{name}: one laser, got {lasers:?}");
        };
        assert_eq!(
            [
                laser.from.x,
                laser.from.y,
                laser.from.z,
                laser.to.x,
                laser.to.y,
                laser.to.z
            ],
            std::array::from_fn(|axis| word(axis) as i32),
            "{name}: laser endpoints"
        );
        assert_eq!(
            (laser.z_adjust, laser.width, laser.duration),
            (word(6) as i32, word(7) as i32, word(11) as i32)
        );
        assert_eq!(
            laser.color,
            crate::sim::combat::laser::LaserColor::Explicit {
                inner: rgb(8),
                outer: rgb(9),
                spread: rgb(10),
            }
        );
        // VERA's one laser model never blinks and always fades.
        assert_eq!((word(12) & 0xff, word(13) & 0xff), (0, 1));
        assert_eq!(
            (laser.fade_start.bits(), laser.fade_end.bits()),
            (word(14), word(15))
        );
    }
}
