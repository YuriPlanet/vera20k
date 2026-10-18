//! Whole original ParticleDraw62CEC0 controls, consumed by the same numeric
//! preparation and ordered GPU transport that draw real Welding children.

use super::*;
use serde_json::Value;

pub(crate) fn native() -> &'static Value {
    crate::test_fixture::json("tools/procedural_drawing_oracle/welding.json")
}

pub(crate) fn int(value: &Value) -> i32 {
    i32::try_from(value.as_i64().expect("native signed integer")).unwrap()
}

pub(crate) fn unsigned(value: &Value) -> u32 {
    u32::try_from(value.as_u64().expect("native unsigned integer")).unwrap()
}

pub(crate) fn array<const N: usize>(value: &Value) -> [i32; N] {
    std::array::from_fn(|index| int(&value[index]))
}

pub(crate) fn f64_bits(value: &Value) -> NativeF64Bits {
    NativeF64Bits::from_bits(
        u64::from_str_radix(value.as_str().unwrap().trim_start_matches("0x"), 16).unwrap(),
    )
}

pub(crate) fn decode_hex(value: &Value) -> Vec<u8> {
    let hex = value.as_str().unwrap();
    assert_eq!(hex.len() % 2, 0);
    hex.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

pub(crate) fn viewport(input: &Value) -> TacticalPointViewport {
    let [left, top, width, height] = array(&input["clip"]);
    let [x, y] = array(&input["camera"]);
    TacticalPointViewport {
        clip: TacticalRect {
            left,
            top,
            width,
            height,
        },
        tactical_offset_x: x,
        tactical_offset_y: y,
        radar_viewport_offset_y: int(&input["radar_viewport_y"]),
        adjust_for_z_multiplier: f64_bits(&input["adjust_for_z_f64_bits"]),
        z_origin_term: i16::try_from(int(&input["z_origin_term"])).unwrap(),
        z_bottom_term: i16::try_from(int(&input["z_bottom_term"])).unwrap(),
    }
}

pub(crate) fn command(point: &Value, draw: &Value, ordinal: usize) -> SparkPointCommand {
    SparkPointCommand {
        world: IVec3::from_array(array(&point["world"])),
        start_rgb: array::<3>(&point["start_rgb"]).map(|value| u8::try_from(value).unwrap()),
        color_index: int(&point["color_index"]),
        color_accumulator: f64_bits(&point["color_accumulator_f64_bits"]),
        damage: int(&point["damage"]),
        gates: SparkDrawGates {
            performance_passed: draw["performance_passed"].as_bool().unwrap(),
            // Unreached later gates cannot change an earlier rejection.
            extra_animations_enabled: draw["detail_passed"].as_bool().unwrap_or(false),
            fog_passed: !draw["fog_result"].as_bool().unwrap_or(false),
        },
        draw_ordinal: ordinal as u64,
    }
}

pub(crate) fn colors(input: &Value) -> Vec<[u8; 3]> {
    input["color_list"]
        .as_array()
        .unwrap()
        .iter()
        .map(|rgb| array::<3>(rgb).map(|value| u8::try_from(value).unwrap()))
        .collect()
}

pub(crate) fn plane(input: &Value, name: &str) -> Vec<u16> {
    let [width, height] = array::<2>(&input["size"]);
    let mut words = vec![
        u16::try_from(int(&input[format!("{name}_default")])).unwrap();
        (width * height) as usize
    ];
    for patch in input[format!("{name}_patches")].as_array().unwrap() {
        let [x, y, value] = array(patch);
        assert!((0..width).contains(&x) && (0..height).contains(&y));
        words[(y * width + x) as usize] = u16::try_from(value).unwrap();
    }
    words
}

pub(crate) fn expected_words(row: &Value) -> Vec<u16> {
    let input = if row["input"]["system"].is_object() {
        &row["input"]["system"]
    } else {
        &row["input"]
    };
    let [width, height] = array::<2>(&input["size"]);
    let mut words =
        vec![u16::try_from(int(&input["background"])).unwrap(); (width * height) as usize];
    for pixel in row["pixels"].as_array().unwrap() {
        let [x, y, value] = array(pixel);
        words[(y * width + x) as usize] = u16::try_from(value).unwrap();
    }
    words
}

pub(crate) fn prepared_points(input: &Value, draws: &Value) -> Vec<PreparedSparkPoint> {
    let colors = colors(input);
    let viewport = viewport(input);
    input["points"]
        .as_array()
        .unwrap()
        .iter()
        .zip(draws.as_array().unwrap())
        .enumerate()
        .filter_map(|(index, (point, draw))| {
            match prepare_spark_point(
                command(point, draw, index),
                &colors,
                viewport,
                SparkPointPlanes::Deferred,
            )
            .unwrap()
            {
                SparkPointPreparation::Ready { point, .. } => Some(point),
                SparkPointPreparation::Rejected(_) => None,
            }
        })
        .collect()
}

#[test]
fn welding_points_match_complete_original_particle_draw() {
    let rows = native()["spark_draw_cases"].as_array().unwrap();
    assert!(!rows.is_empty());
    for row in rows {
        let input = &row["input"];
        let [width, height] = array::<2>(&input["size"]).map(|value| value as usize);
        let a = plane(input, "a");
        let z = plane(input, "z");
        let colors = colors(input);
        let viewport = viewport(input);
        let mut output = vec![int(&input["background"]) as u16; width * height];
        let points = input["points"].as_array().unwrap();
        let draws = row["draws"].as_array().unwrap();
        assert_eq!(points.len(), draws.len());
        for (ordinal, (point, draw)) in points.iter().zip(draws).enumerate() {
            let command = command(point, draw, ordinal);
            let result = resolve_spark_point(
                command,
                &colors,
                TacticalCompatFrame {
                    a_plane: WordPlane {
                        words: &a,
                        width,
                        height,
                        pitch_words: width,
                        row_origin: 0,
                    },
                    z_plane: WordPlane {
                        words: &z,
                        width,
                        height,
                        pitch_words: width,
                        row_origin: 0,
                    },
                    viewport,
                    pixel_format: crate::render::native_surface_format::RGB565,
                },
            )
            .unwrap();
            let actual = match result {
                PointResolution::Rejected(_) => Value::Null,
                PointResolution::Write(write) => {
                    assert_eq!(write.draw_ordinal, ordinal as u64);
                    assert_eq!(write.byte_width, 2);
                    output[write.screen.y as usize * width + write.screen.x as usize] =
                        write.packed_value;
                    serde_json::json!({"screen":write.screen.to_array(),"packed_value":write.packed_value})
                }
            };
            assert_eq!(actual, draw["write"], "{} point{ordinal}", input["name"]);
            if let SparkPointPreparation::Ready {
                point: prepared, ..
            } =
                prepare_spark_point(command, &colors, viewport, SparkPointPlanes::Deferred).unwrap()
            {
                for (key, actual) in [
                    ("screen", serde_json::json!(prepared.screen.to_array())),
                    ("candidate_z", serde_json::json!(prepared.candidate_z)),
                    ("interpolated_rgb", serde_json::json!(prepared.rgb)),
                ] {
                    if !draw[key].is_null() {
                        assert_eq!(actual, draw[key], "{} {key}", input["name"]);
                    }
                }
            }
        }
        assert_eq!(
            output,
            expected_words(row),
            "{} full surface",
            input["name"]
        );
    }
}
