use super::*;

/// FFmpeg's own xBR output at each factor, for an image whose clear pixels
/// hold a colour FFmpeg was given as black (`tools/xbr_ffmpeg.meta.json`).
#[test]
fn matches_ffmpeg_at_each_factor() {
    let golden = crate::test_fixture::json("tools/xbr_ffmpeg.json");
    let width = golden["width"].as_u64().unwrap() as usize;
    let height = golden["height"].as_u64().unwrap() as usize;
    let rgba = hex_rows(&golden["rgba"]);
    for factor in 2..=4 {
        let scaled = scale(&rgba, width, height, factor);
        let rgb: Vec<u8> = scaled
            .chunks_exact(4)
            .flat_map(|pixel| pixel[..3].to_vec())
            .collect();
        assert_eq!(
            rgb,
            hex_rows(&golden["scaled"][factor.to_string()]),
            "at {factor}x"
        );
        for (index, pixel) in scaled.chunks_exact(4).enumerate() {
            let (x, y) = (index % (width * factor), index / (width * factor));
            let source = (y / factor * width + x / factor) * 4;
            assert_eq!(pixel[3], rgba[source + 3], "alpha at ({x}, {y}), {factor}x");
        }
    }
}

fn hex_rows(rows: &serde_json::Value) -> Vec<u8> {
    let hex: String = rows
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row.as_str().unwrap())
        .collect();
    (0..hex.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(&hex[at..at + 2], 16).unwrap())
        .collect()
}
