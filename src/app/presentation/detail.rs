//! Process-lived presentation detail state from active-retail gamemd.exe.
//!
//! Logic55AFB0 counts visits; Throttle55E33B publishes them every 60 wall-clock
//! buckets (timeGetTime >> 4). GameGetMinFrameRate55AF60 owns the hysteresis
//! shared by Anim and LaserDraw. This state does not affect simulation.
//! Executed controls: tools/spatial_oracle/building_prism.{py,json,meta.json},
//! `laser_fps` and `laser_detail_selection`; binary SHA256
//! 1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c.

use crate::rules::ruleset::DetailRules;
use crate::sim::timer::CdTimer;

/// One owner for the native process globals, retained across scenario loads.
#[derive(Debug)]
pub(crate) struct DetailState {
    logic_visits: u32,
    frame_rate: u32,
    total_logic_visits: u32,
    sample_count: u32,
    sample_timer: CdTimer,
    initialized: bool,
    reduced: bool,
    minimum: u32,
    buffer: u32,
}

/// Immutable diagnostics; `minimum` is the selected global, not a query of
/// the mutative hysteresis function.
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub(crate) struct DetailObservation {
    frame_rate: u32,
    minimum: u32,
    buffer: u32,
    reduced: bool,
    logic_visits: u32,
    sample_start: i32,
    sample_duration: i32,
    initialized: bool,
}

impl Default for DetailState {
    fn default() -> Self {
        let rules = DetailRules::default();
        Self {
            // ABCD40..ABCD50 and ABCD88..ABCD94 are loader-zero BSS.
            logic_visits: 0,
            frame_rate: 0,
            total_logic_visits: 0,
            sample_count: 0,
            sample_timer: CdTimer::from_raw(0, 0),
            initialized: false,
            reduced: false,
            // File-backed 829FF4/829FF8 are 15/5, matching Rules defaults.
            minimum: rules.min_frame_rate_normal as u32,
            buffer: rules.buffer_zone_width as u32,
        }
    }
}

impl DetailState {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Fill_In_Data6850F5..685110 calls 55AF40/55AF50 with retained Rules
    /// values. These setters preserve the counter, sample timer and latch.
    pub(crate) fn configure_normal(&mut self, rules: &DetailRules) {
        self.minimum = rules.min_frame_rate_normal as u32;
        self.buffer = rules.buffer_zone_width as u32;
    }

    /// Logic55AFB3..55AFD3 increments ABCD40 before the Logic body runs.
    /// A visit counts even when that body exits without committing a frame.
    pub(crate) fn record_logic_visit(&mut self) {
        self.logic_visits = self.logic_visits.wrapping_add(1);
    }

    /// Throttle55E33B..55E403 publishes at most one sample per service pass.
    /// The caller supplies its existing app clock at this boundary, including
    /// passes that did not admit Logic. Native 6C8C40 wraps milliseconds before
    /// shifting; its signed timer consequently retains the clock-wrap delay.
    pub(crate) fn throttle_tail(&mut self, wall_ms: u64) {
        let bucket = ((wall_ms as u32) >> 4) as i32;
        if !self.initialized {
            self.initialized = true;
            self.sample_timer.start(bucket, 0);
        }
        if !self.sample_timer.expired(bucket) {
            return;
        }

        self.frame_rate = self.logic_visits;
        self.logic_visits = 0;
        self.total_logic_visits = self.total_logic_visits.wrapping_add(self.frame_rate);
        self.sample_count = self.sample_count.wrapping_add(1);
        if self.total_logic_visits > i32::MAX as u32 {
            self.total_logic_visits = 0;
            self.sample_count = 0;
        }
        self.sample_timer.start(bucket, 60);
    }

    /// The last published Logic count (ABCD44), without averaging or catch-up.
    pub(crate) fn frame_rate(&self) -> u32 {
        self.frame_rate
    }

    pub(crate) fn observation(&self) -> DetailObservation {
        DetailObservation {
            frame_rate: self.frame_rate,
            minimum: self.minimum,
            buffer: self.buffer,
            reduced: self.reduced,
            logic_visits: self.logic_visits,
            sample_start: self.sample_timer.start_frame(),
            sample_duration: self.sample_timer.duration(),
            initialized: self.initialized,
        }
    }

    /// GameGetMinFrameRate55AF60: each consumer query can update ABCD50.
    /// Signed INI values retain their bits; comparisons and addition here are
    /// unsigned. In particular, overflowing min+buffer is not a stable query
    /// to cache across consumers.
    pub(crate) fn minimum_frame_rate(&mut self) -> u32 {
        if self.reduced {
            let threshold = self.minimum.wrapping_add(self.buffer);
            if self.frame_rate < threshold {
                return threshold;
            }
            self.reduced = false;
            return self.minimum;
        }
        if self.frame_rate >= self.minimum {
            return self.minimum;
        }
        self.reduced = true;
        self.minimum.wrapping_add(self.buffer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn corpus() -> Value {
        serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/building_prism.json",
        ))
        .unwrap()
    }

    fn raw(input: &Value, key: &str, default: u32) -> u32 {
        input[key].as_u64().map_or(default, |value| value as u32)
    }

    fn initial_state(input: &Value) -> DetailState {
        let default = DetailState::new();
        DetailState {
            logic_visits: raw(input, "count", default.logic_visits),
            frame_rate: raw(input, "fps", default.frame_rate),
            total_logic_visits: raw(input, "total", default.total_logic_visits),
            sample_count: raw(input, "buckets", default.sample_count),
            sample_timer: input["timer"]
                .as_array()
                .map_or(default.sample_timer, |timer| {
                    CdTimer::from_raw(
                        timer[0].as_u64().unwrap() as i32,
                        timer[2].as_u64().unwrap() as i32,
                    )
                }),
            initialized: input["initialized"].as_bool().unwrap_or(false),
            reduced: input["reduced"].as_bool().unwrap_or(false),
            minimum: raw(input, "minimum", default.minimum),
            buffer: raw(input, "buffer", default.buffer),
        }
    }

    #[test]
    fn native_logic_sampling_and_detail_query_histories() {
        let corpus = corpus();
        let rows = corpus["laser_fps"].as_array().unwrap();
        assert_eq!(rows.len(), 7);
        for row in rows {
            let mut state = initial_state(&row["input"]);
            for (index, expected) in row["history"].as_array().unwrap().iter().enumerate() {
                let input = &expected["input"];
                for _ in 0..raw(input, "logic_visits", 0) {
                    state.record_logic_visit();
                }
                state.throttle_tail(input["milliseconds"].as_u64().unwrap());
                if let Some(configure) = input["configure"].as_array() {
                    state.configure_normal(&DetailRules {
                        min_frame_rate_normal: configure[0].as_i64().unwrap() as i32,
                        buffer_zone_width: configure[1].as_i64().unwrap() as i32,
                        ..DetailRules::default()
                    });
                }
                let actual = json!({
                    "threshold": state.minimum_frame_rate(),
                    "count": state.logic_visits,
                    "fps": state.frame_rate(),
                    "total": state.total_logic_visits,
                    "buckets": state.sample_count,
                    "timer": [state.sample_timer.start_frame() as u32,
                              state.sample_timer.duration() as u32],
                    "initialized": u8::from(state.initialized),
                    "reduced": u8::from(state.reduced),
                    "minimum": state.minimum,
                    "buffer": state.buffer,
                });
                for (key, value) in actual.as_object().unwrap() {
                    assert_eq!(
                        value, &expected[key],
                        "{} step {index}, field {key}",
                        row["input"]["name"]
                    );
                }
            }
        }
    }

    #[test]
    fn native_fill_in_data_selects_normal_rules_without_resetting_process_state() {
        let corpus = corpus();
        for row in corpus["laser_detail_selection"].as_array().unwrap() {
            let rules = &row["rules_normal_movie_buffer"];
            let mut state = DetailState {
                logic_visits: 3,
                frame_rate: 44,
                total_logic_visits: 80,
                sample_count: 2,
                reduced: true,
                ..DetailState::new()
            };
            state.configure_normal(&DetailRules {
                min_frame_rate_normal: rules[0].as_i64().unwrap() as i32,
                min_frame_rate_movie: rules[1].as_i64().unwrap() as i32,
                buffer_zone_width: rules[2].as_i64().unwrap() as i32,
            });
            let expected = &row["history"][0];
            assert_eq!(expected["caller"], "fill_in_data");
            assert_eq!(state.minimum, expected["minimum"].as_u64().unwrap() as u32);
            assert_eq!(state.buffer, expected["buffer"].as_u64().unwrap() as u32);
            assert_eq!(
                (
                    state.logic_visits,
                    state.frame_rate(),
                    state.total_logic_visits,
                    state.sample_count,
                    state.reduced
                ),
                (3, 44, 80, 2, true)
            );
        }
    }
}
