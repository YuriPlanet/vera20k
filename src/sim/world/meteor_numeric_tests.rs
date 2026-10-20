//! Original constructor arithmetic and meteor compensation; declared scalar
//! and terrain inputs are bounded by trigger_meteor_numeric.meta.json.
use super::*;
use crate::rules::{art_data::ArtRegistry, ini_parser::IniFile};
use crate::sim::{
    anim_class::{AnimWorldCoord, anim_constructor_draws},
    rng::SimRng,
};
use crate::util::sha256::sha256_hex;

fn golden() -> &'static Value {
    crate::test_fixture::json("tools/spatial_oracle/trigger_meteor_numeric.json")
}

fn input_rng(row: &Value) -> SimRng {
    let seeded = SimRng::new(row["input"]["seed"].as_u64().unwrap());
    let mut hex = seeded.native_state_hex();
    for change in row["changes"].as_array().unwrap() {
        let index = change["index"].as_u64().unwrap() as usize;
        let bytes = (change["value"].as_u64().unwrap() as u32).to_le_bytes();
        let replacement: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
        let offset = (12 + index * 4) * 2;
        hex.replace_range(offset..offset + 8, &replacement);
    }
    assert_eq!(
        sha256_hex(hex.as_bytes()),
        row["expected"]["rng_before_sha256"].as_str().unwrap()
    );
    SimRng::from_native_state_hex_for_test(&hex)
}

fn construct(row: &Value) -> BounceState {
    let input = &row["input"];
    let exact = |name: &str| NativeF64Bits::from_bits(input[name].as_f64().unwrap().to_bits());
    let coord: [i32; 3] = std::array::from_fn(|i| input["coord"][i].as_i64().unwrap() as i32);
    let mut rng = input_rng(row);
    let body = {
        let rate = input["random_rate"]
            .as_array()
            .map(|values| format!("RandomRate={},{}\n", values[0], values[1]))
            .unwrap_or_default();
        let art = ArtRegistry::from_ini(&IniFile::from_str(&format!("[T]\nIsMeteor=yes\n{rate}")));
        let mut config = art.anim_runtime_config("T").unwrap().clone();
        config.elasticity = exact("elasticity");
        config.max_xy_vel = exact("max_xy");
        config.min_z_vel = exact("min_z");
        anim_constructor_draws(
            &config,
            AnimWorldCoord {
                x: coord[0],
                y: coord[1],
                z: coord[2],
            },
            &mut rng,
        )
        .unwrap()
        .bounce
        .unwrap()
    };
    assert_eq!(
        sha256_hex(rng.native_state_hex().as_bytes()),
        row["expected"]["rng_after_sha256"].as_str().unwrap()
    );
    body
}

#[test]
fn meteor_constructors_match_native_draws_float_stores_and_integer_placement() {
    assert_eq!(golden()["schema"], 1);
    assert_eq!(
        golden()["native_sha256"],
        "3e81a61775d2745d1dabe397325ef663cd994ffc194da4e998e3bf5d2d308600"
    );
    let rows = golden()["constructors"].as_array().unwrap();
    assert_eq!(rows.len(), 8);
    for row in rows {
        let body = construct(row);
        let expected = &row["expected"];
        let fields = &expected["body"];
        let triple = |name: &str| {
            std::array::from_fn::<_, 3, _>(|i| fields[name][i].as_u64().unwrap() as u32)
        };
        let quad = |name: &str| {
            std::array::from_fn::<_, 4, _>(|i| fields[name][i].as_u64().unwrap() as u32)
        };
        assert_eq!(
            body.position_leptons().to_array(),
            std::array::from_fn::<_, 3, _>(|i| expected["location"][i].as_i64().unwrap() as i32),
            "{row}"
        );
        assert_eq!(
            body.position().map(|value| value.bits()),
            triple("position_bits"),
            "{row}"
        );
        assert_eq!(
            body.velocity().map(|value| value.bits()),
            triple("velocity_bits"),
            "{row}"
        );
        assert_eq!(
            body.orientation().map(|value| value.bits()),
            quad("orientation_bits"),
            "{row}"
        );
        assert_eq!(
            body.rotation().map(|value| value.bits()),
            quad("rotation_bits"),
            "{row}"
        );
        assert_eq!(
            body.elasticity().bits(),
            fields["elasticity_bits"].as_u64().unwrap()
        );
        assert_eq!(
            body.gravity().bits(),
            fields["gravity_bits"].as_u64().unwrap()
        );
        assert_eq!(
            body.angular_velocity_magnitude().bits(),
            fields["clamp_bits"].as_u64().unwrap()
        );
    }
}

#[test]
fn meteor_update_compensation_matches_original_flights_on_declared_crop() {
    let rows = golden()["flights"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    let sim = flight_sim("flat2", 40);
    let terrain = ResolvedBounceTerrain {
        sim: &sim,
        rules: None,
    };
    for row in rows {
        let constructor =
            &golden()["constructors"][row["constructor_row"].as_u64().unwrap() as usize];
        let mut body = construct(constructor);
        for (tick, expected) in row["ticks"].as_array().unwrap().iter().enumerate() {
            let outcome = outcome_code(body.update(&terrain).unwrap());
            body.compensate_meteor_gravity().unwrap();
            assert_eq!(outcome, expected[0].as_i64().unwrap(), "tick{tick}: {row}");
            assert_eq!(
                body.position_leptons().to_array(),
                std::array::from_fn::<_, 3, _>(|i| expected[1 + i].as_i64().unwrap() as i32),
                "tick{tick}"
            );
            assert_eq!(
                body.position().map(|value| value.bits()),
                std::array::from_fn::<_, 3, _>(|i| expected[4 + i].as_u64().unwrap() as u32),
                "tick{tick}"
            );
            // The existing flat-reflection kernel admits signed-zero deltas;
            // all nonzero velocity values still compare exactly as f32.
            assert_eq!(
                body.velocity().map(|value| f32::from_bits(value.bits())),
                std::array::from_fn::<_, 3, _>(|i| f32::from_bits(
                    expected[7 + i].as_u64().unwrap() as u32
                )),
                "tick{tick}"
            );
        }
    }
}
