//! The ripple tables and spiral functions against `tools/superweapon_oracle.json`
//! `ion_blast_ripple` (the original generator `0x0053D330`, `0x0053D8E0` and
//! `0x0053D960` run natively).

use super::*;
use crate::util::fnv::{FNV1A64_OFFSET_BASIS, fnv1a64_fold_bytes};
use serde_json::Value;

fn native() -> Value {
    let oracle: Value =
        serde_json::from_str(crate::test_fixture::text("tools/superweapon_oracle.json")).unwrap();
    oracle["ion_blast_ripple"].clone()
}

fn int(value: &Value) -> i32 {
    value.as_i64().unwrap() as i32
}

/// Every frame's bytes hash, count and value set as the original's; the
/// retail sine table is the repository's checked copy.
#[test]
fn generated_frames_match_the_original() {
    let native = native();
    let frames = RippleFrames::generate(TrigTable::embedded());
    let rows = native["frames"].as_array().unwrap();
    assert_eq!(rows.len(), FRAME_COUNT);
    let frames = frames
        .bytes()
        .as_chunks::<{ FRAME_WIDTH * FRAME_HEIGHT }>()
        .0;
    for (n, (row, frame)) in rows.iter().zip(frames).enumerate() {
        let digest = format!("{:#018x}", fnv1a64_fold_bytes(FNV1A64_OFFSET_BASIS, frame));
        let written = frame.iter().filter(|&&byte| byte != 0xff).count();
        let mut values: Vec<u8> = frame.to_vec();
        values.sort_unstable();
        values.dedup();
        let expected: Vec<u8> = row["values"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_u64().unwrap() as u8)
            .collect();
        assert_eq!(
            (digest.as_str(), written, values),
            (
                row["fnv1a64"].as_str().unwrap(),
                row["written"].as_u64().unwrap() as usize,
                expected
            ),
            "frame {n}"
        );
    }
}

/// The ring step is the start-up rounding of the initializer, and the
/// process's truncation gives the next lower double.
#[test]
fn ring_step_is_the_startup_initializers_value() {
    let native = native();
    let bits = |fpcw: &str| -> Vec<String> {
        native["step_bits"][fpcw]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_owned())
            .collect()
    };
    let step = format!("{:#018x}", RING_STEP.bits());
    assert_eq!(bits("0x027F"), [step.clone(), step]);
    let chopped = format!("{:#018x}", RING_STEP.bits() - 1);
    assert_eq!(bits("0x0E7F"), [chopped.clone(), chopped]);
}

#[test]
fn spiral_functions_match_the_original() {
    let native = native();
    for row in native["spiral_points"].as_array().unwrap() {
        let index = int(&row[0]);
        assert_eq!(
            spiral_point(index),
            (int(&row[1]), int(&row[2])),
            "index {index}"
        );
    }
    for row in native["spiral_indexes"].as_array().unwrap() {
        let (x, y) = (int(&row[0]), int(&row[1]));
        assert_eq!(spiral_index(x, y), int(&row[2]), "({x}, {y})");
    }
}

/// Every positive byte the generator writes moves a pixel straight down by
/// two to ten rows, so the draw reads rows its blast has not yet written.
#[test]
fn generated_bytes_read_rows_below() {
    let frames = RippleFrames::generate(TrigTable::embedded());
    let mut moves: Vec<[i32; 2]> = frames
        .bytes()
        .iter()
        .filter_map(|&byte| displacement(byte))
        .collect();
    moves.sort_unstable();
    moves.dedup();
    assert_eq!(moves, [[0, 2], [0, 4], [0, 6], [0, 8], [0, 10]]);
}
