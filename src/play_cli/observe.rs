//! The versioned responses from the headless match.

use crate::map::entities::EntityCategory;
use crate::sim::production::{BuildQueueState, ProductionCategory};
use serde::Serialize;

pub const SCHEMA: &str = "vera20k.play.v1";

#[derive(Debug, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Event {
    Ready {
        schema: &'static str,
        map: String,
        seed: u32,
        observation: Observation,
    },
    State {
        observation: Observation,
    },
    Queued {
        tick: u64,
    },
    BuildOptions {
        options: Vec<BuildOption>,
    },
    Placement {
        valid: bool,
        in_build_area: bool,
        foundation_visible: bool,
        reason: Option<&'static str>,
    },
    Cell {
        x: u16,
        y: u16,
        revealed: bool,
        visible: bool,
        level: Option<u8>,
        land_type: Option<u8>,
    },
    Error {
        message: String,
        fatal: bool,
    },
    Finished {
        reason: FinishReason,
        observation: Observation,
    },
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    Quit,
    Eof,
    TickLimit,
    Victory,
    Defeat,
    MatchEnded,
}

#[derive(Debug, Serialize)]
pub struct Observation {
    pub tick: u64,
    pub binary_frame: u32,
    pub state_hash: String,
    pub map_width: u16,
    pub map_height: u16,
    pub house: House,
    pub units: Vec<Object>,
    /// Non-owned objects in visible cells. This is a diagnostic view of those cells.
    pub contacts: Vec<Object>,
    pub factories: Vec<Factory>,
}

#[derive(Debug, Serialize)]
pub struct House {
    pub name: String,
    pub credits: i32,
    pub spent_credits: i32,
    pub power_output: i32,
    pub power_drain: i32,
}

#[derive(Debug, Serialize)]
pub struct Object {
    pub id: u64,
    pub type_name: String,
    pub owner: String,
    pub category: EntityCategory,
    pub x: u16,
    pub y: u16,
    pub health: i32,
}

#[derive(Debug, Serialize)]
pub struct Factory {
    pub category: ProductionCategory,
    pub type_name: String,
    pub progress: u16,
    pub state: BuildQueueState,
    pub queue: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct BuildOption {
    pub type_name: String,
    pub category: ProductionCategory,
    pub cost: i32,
}
