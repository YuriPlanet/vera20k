use super::*;
use serde_json::{Value, json};

pub(crate) fn native() -> &'static Value {
    crate::test_fixture::json("tools/procedural_drawing_oracle/radial_indicator.json")
}

pub(crate) fn integer(value: &Value) -> i32 {
    value.as_i64().unwrap() as i32
}
pub(crate) fn point<const N: usize>(value: &Value) -> [i32; N] {
    std::array::from_fn(|i| integer(&value[i]))
}

pub(crate) fn draws(row: &Value) -> Vec<RadialIndicatorDraw> {
    let camera: [i32; 2] = point(&row["input"]["camera"]);
    let clip: [i32; 4] = point(&row["input"]["clip"]);
    row["radials"]
        .as_array()
        .unwrap()
        .iter()
        .map(|radial| {
            let truncate_radius = radial["truncate_radius"].as_bool().unwrap();
            let coord: [i32; 3] = point(&radial["coord"]);
            let (x, y) =
                crate::util::lepton::absolute_leptons_to_screen(coord[0], coord[1], coord[2]);
            RadialIndicatorDraw {
                center: [
                    x as i32 - camera[0] + clip[0],
                    y as i32 - camera[1] - 15 + clip[1],
                ],
                radius: f32::from_bits(
                    u32::from_str_radix(radial["radius_f32_bits"].as_str().unwrap(), 16).unwrap(),
                ),
                truncate_radius,
                pixel_radius: radial["pixel_radius"].as_bool().unwrap(),
                rgb: point::<3>(&radial["rgb"]).map(|v| v as u8),
                fade: radial["adjust_color"].as_bool().unwrap(),
                rays: radial["draw_line"].as_bool().unwrap(),
                frame: integer(&row["input"]["frame"]),
            }
        })
        .collect()
}

#[test]
fn original_cubic_and_tangent_tables_and_signed_tangent_returns_are_consumed() {
    use crate::util::native_x87::NativeF64Bits;
    let lut: Vec<_> = crate::render::surface_line::CUBIC_TABLE
        .chunks_exact(4)
        .map(|b| i32::from_le_bytes(b.try_into().unwrap()))
        .collect();
    assert_eq!(json!(lut), native()["cubic_lut"]["values"]);
    let tangent: Vec<_> = crate::util::native_trig::RETAIL_TANGENT_TABLE
        .chunks_exact(4)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
        .collect();
    assert_eq!(json!(tangent), native()["tangent"]["u32_bits"]);
    for row in native()["tangent"]["controls"].as_array().unwrap() {
        let bits = u64::from_str_radix(row["angle_f64_bits"].as_str().unwrap(), 16).unwrap();
        let output = crate::util::native_trig::native_tan_from_table(
            X::load_f64(NativeF64Bits::from_bits(bits)).unwrap(),
        );
        assert_eq!(
            format!("{:016X}", X::store_f64(output).unwrap().bits()),
            row["returned_f64_bits"].as_str().unwrap()
        );
    }
}

#[test]
fn radial_geometry_colors_and_call_order_match_original_executable() {
    for row in native()["scenes"].as_array().unwrap() {
        let clip: [i32; 4] = point(&row["input"]["clip"]);
        let mut actual = Vec::new();
        for draw in draws(row) {
            draw.draws(clip,|command|actual.push(match command {
                SurfaceDraw::Ellipse {center,radius,word} => json!({"kind":"ellipse","center":center,"radius":radius,"clip":clip,"color":word}),
                SurfaceDraw::OpaqueRect{..} | SurfaceDraw::Point(_) | SurfaceDraw::MagneticWave(_) => unreachable!("radial indicators emit ellipses and rays"),
                SurfaceDraw::Line(line) => {
                    let SurfaceLineBlend::Antialiased {rgb,intensity}=line.blend else {panic!("radial uses4BC750");};
                    json!({"kind":"ray","start":line.from,"end":line.to,"clip":clip,"rgb":rgb,"z":line.z_adjust,"write_z":false,"channels":[true,true,true],"strength_f32_bits":format!("{:08X}",intensity.bits())})
                }
            }));
        }
        let expected: Vec<Value> = row["commands"]
            .as_array()
            .unwrap()
            .iter()
            .map(|command| {
                let mut command = command.clone();
                for key in ["actor", "store_count", "touched_pixels"] {
                    command.as_object_mut().unwrap().remove(key);
                }
                command
            })
            .collect();
        assert_eq!(actual, expected, "{}", row["input"]["name"]);
    }
}

#[test]
fn native_superweapon_radius_prefix_controls_are_consumed() {
    for control in native()["superweapon_range"]["primitive_radius_controls"]
        .as_array()
        .unwrap()
    {
        let bits =
            u32::from_str_radix(control["input_radius_f32_bits"].as_str().unwrap(), 16).unwrap();
        let radius = f32::from_bits(bits);
        let truncate = control["truncate_input"].as_bool().unwrap();
        assert_eq!(
            radius_pixels(radius, truncate),
            Some(control["output_radius_pixels"].as_i64().unwrap() as i32),
            "{}",
            control["name"]
        );
    }
}

#[test]
fn invalid_and_oversized_radials_emit_no_surface_work() {
    for radius in [
        f32::NAN,
        f32::INFINITY,
        f32::NEG_INFINITY,
        -1.0,
        1e38,
        4294967296.0,
    ] {
        for pixel_radius in [false, true] {
            let draw = RadialIndicatorDraw {
                center: [100, 100],
                radius,
                truncate_radius: true,
                pixel_radius,
                rgb: [255; 3],
                fade: false,
                rays: false,
                frame: 0,
            };
            draw.draws([0, 0, 200, 200], |_| {
                panic!("unsupported radius emitted work")
            });
        }
    }
    let oversized = RadialIndicatorDraw {
        center: [100, 100],
        radius: 1000.0,
        truncate_radius: false,
        pixel_radius: false,
        rgb: [255; 3],
        fade: false,
        rays: false,
        frame: 0,
    };
    assert!(radius_pixels(oversized.radius, false).is_some());
    oversized.draws([0, 0, 200, 200], |_| {
        panic!("oversized radial emitted work")
    });
    crate::render::surface_line::ellipse(
        [100, 100],
        [MAX_ELLIPSE_RADIUS_PIXELS + 1, 512],
        [0, 0, 200, 200],
        |_| panic!("oversized ellipse bypassed the shared work budget"),
    );
}
