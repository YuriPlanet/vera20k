//! Consumed original6591B0/659650/659AC0/65A1B0 outputs. Prepared inputs
//! remain explicit; no Rust output supplies a golden.
use super::*;
use serde_json::Value;

pub(crate) fn corpus() -> &'static Value {
    crate::test_fixture::json("tools/procedural_drawing_oracle/rad_beam.json")
}
pub(crate) fn integer(v: &Value) -> i32 {
    v.as_i64().unwrap().try_into().unwrap()
}
pub(crate) fn array<const N: usize>(v: &Value) -> [i32; N] {
    std::array::from_fn(|i| integer(&v[i]))
}
pub(crate) fn coord(v: &Value) -> ProjectileCoord {
    let [x, y, z] = array(v);
    ProjectileCoord::new(x, y, z)
}
pub(crate) fn bits(v: &Value) -> u64 {
    let text = v.as_str().unwrap();
    let bytes = std::array::from_fn(|i| u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).unwrap());
    u64::from_le_bytes(bytes)
}
pub(crate) fn birth(v: &Value) -> RadiationBeamBirth {
    RadiationBeamBirth {
        frame: 200,
        from: coord(&v["source"]),
        to: coord(&v["target"]),
        z_adjust: integer(&v["z_adjust"]),
        color: array::<3>(&v["rgb"]).map(|v| v.try_into().unwrap()),
        house: None,
        house_color: false,
        duration: integer(&v["duration"]),
        amplitude: NativeF64Bits::from_bits(bits(&v["amplitude_f64_le_hex"])),
    }
}
pub(crate) fn viewport(row: &Value) -> SurfaceLineViewport {
    let mut camera: [i32; 2] = array(&row["viewport"]["camera"]);
    camera[1] += 15; // Shared VERA world-row convention, as Laser/EBolt fixtures.
    SurfaceLineViewport {
        camera,
        clip: array(&row["viewport"]["clip"]),
        z_origin_y: 0,
        zoom: 1.0,
    }
}
pub(crate) fn drawing_rows() -> impl Iterator<Item = &'static Value> {
    corpus()["geometry_rows"]
        .as_array()
        .unwrap()
        .iter()
        .chain(
            crate::test_fixture::json("tools/procedural_drawing_oracle/chrono_beam.json")
                ["geometry_rows"]
                .as_array()
                .unwrap()
                .iter(),
        )
        // Ordinary SpawnRadBeam never disables its drawing flag. The
        // retained disabled control documents that separate caller domain.
        .filter(|row| row["input"]["draw_enabled"] != false)
}
pub(crate) fn lines(row: &Value) -> Vec<SurfaceLine> {
    let birth = birth(&row["supplied_birth"]);
    let geometry = BeamGeometry::new(birth.from, birth.to);
    let mut lines = Vec::new();
    geometry.lines(
        birth,
        integer(&row["before_progress"]),
        10,
        viewport(row),
        |line| lines.push(line),
    );
    lines
}

#[test]
fn original_spacing_signed_division_and_wrapping_length_are_consumed() {
    for row in corpus()["fps_rows"].as_array().unwrap() {
        let input = &row["input"];
        let spacing = segment_spacing(
            input["current_u32"].as_u64().unwrap() as u32,
            integer(&input["requested_i32"]),
        );
        assert_eq!(
            (spacing as f64).to_bits(),
            bits(&row["spacing_f64_le_hex"]),
            "{}",
            input["name"]
        );
    }
    for row in corpus()["division_rows"].as_array().unwrap() {
        assert_eq!(
            divide_coordinate(
                array(&row["input"]["value"]),
                integer(&row["input"]["divisor"])
            ),
            array::<3>(&row["result"])
        );
    }
    for row in corpus()["length_rows"].as_array().unwrap() {
        let geometry =
            BeamGeometry::new(ProjectileCoord::new(0, 0, 0), coord(&row["input"]["delta"]));
        assert_eq!(geometry.delta, array::<3>(&row["actual_delta"]));
        assert_eq!(
            geometry.length().bits(),
            bits(&row["length_f64_le_hex"]),
            "{}",
            row["input"]["name"]
        );
    }
}

#[test]
fn original_world_segments_preclipped_xy_depth_and_rgb565_match() {
    for row in drawing_rows() {
        let name = &row["input"]["name"];
        let birth = birth(&row["supplied_birth"]);
        let geometry = BeamGeometry::new(birth.from, birth.to);
        let frame = StraightFrame::new(geometry, birth, integer(&row["before_progress"]), 10);
        for segment in row["after"]["geometry"].as_array().unwrap() {
            let actual = frame.segment(integer(&segment["index"]));
            assert_eq!(
                (actual.from, actual.to, actual.color),
                (
                    coord(&segment["from_coord"]),
                    coord(&segment["to_coord"]),
                    array::<3>(&segment["rgb"]).map(|v| v as u8)
                ),
                "{name} segment{}",
                segment["index"]
            );
        }
        let expected = row["after"]["segments"]
            .as_array()
            .unwrap()
            .iter()
            .map(|line| SurfaceLine {
                from: array(&line["from_point"]),
                to: array(&line["to_point"]),
                z_adjust: [integer(&line["z_start"]), integer(&line["z_end"])],
                blend: SurfaceLineBlend::Replace(integer(&line["packed_word"]) as u16),
            })
            .collect::<Vec<_>>();
        assert_eq!(lines(row), expected, "{name}");
    }
}
