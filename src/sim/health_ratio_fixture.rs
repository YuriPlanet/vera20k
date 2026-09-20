//! Test-only inputs and native outputs from the original 5F5C60 caller corpus.
//! This module deliberately contains no health comparison implementation.

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub(crate) struct Input {
    pub current: i32,
    pub strength: i32,
    yellow_bits: String,
    red_bits: String,
}

impl Input {
    pub fn yellow(&self) -> f64 {
        decode(&self.yellow_bits)
    }

    pub fn red(&self) -> f64 {
        decode(&self.red_bits)
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct Output {
    pub navigation_category: u8,
    pub occupied_body_frames: Vec<BodyFrame>,
    pub garrison_red: bool,
    pub damage_fire_occupied: bool,
    pub damage_fire_ordinary: bool,
    pub bunker_wall_damaged: bool,
    pub generic_art_damaged: bool,
    pub drive_speed_bits: String,
    pub ship_speed_bits: String,
    pub refinery_special_damaged: bool,
    pub refinery_active_damaged: bool,
    pub cloak_above_red: bool,
    pub smoke_above_yellow: bool,
}

#[derive(Debug, Deserialize)]
pub(crate) struct BodyFrame {
    pub occupants: u32,
    pub tech_level: i32,
    pub frame: u16,
}

#[derive(Debug, Deserialize)]
pub(crate) struct Row {
    pub input: Input,
    pub output: Output,
}

pub(crate) fn decode(bits: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(bits, 16).expect("native f64 bits"))
}

pub(crate) fn rows() -> Vec<Row> {
    #[derive(Deserialize)]
    struct Corpus {
        rows: Vec<Row>,
    }
    let corpus: Corpus = serde_json::from_str(include_str!(
        "../../tools/spatial_oracle/health_ratio_predicates.json"
    ))
    .expect("original health ratio caller corpus");
    assert_eq!(corpus.rows.len(), 103);
    corpus.rows
}
