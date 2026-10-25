//! ProcessNetwork's local timing producer. All resulting shared changes travel
//! through the existing OutList, reliable exchange and EventClass dispatcher.

use std::num::NonZeroU8;

use super::LiveCommandMode;
use crate::net::reliable::RetryTiming;
use crate::sim::command::TimingRecord;

#[derive(Debug, Default)]
pub(super) struct LiveTiming {
    precalculated: Option<(u16, u16)>,
    process_milliseconds: u32,
    process_samples: u32,
    processing_started: Option<u32>,
}

impl LiveTiming {
    pub fn begin_processing(&mut self, now_millis: u32) {
        self.processing_started = Some(now_millis);
    }

    pub fn finish_processing(&mut self, now_millis: u32) {
        if let Some(started) = self.processing_started.take() {
            self.record_processing_time(now_millis.wrapping_sub(started));
        }
    }
    pub fn configure_precalculated(&mut self, ahead: i32, fps: i32) {
        self.precalculated = (ahead != 0 || fps != 0).then_some((ahead as u16, fps as u16));
    }

    /// MainTick55D3DA/55DE0D measures the fresh Logic visit before Queue_AI.
    /// A modal House outcome wait remains part of that Logic visit and is
    /// clamped to1000ms. A resumed network wait never adds another sample.
    pub fn record_processing_time(&mut self, milliseconds: u32) {
        self.process_milliseconds = self
            .process_milliseconds
            .wrapping_add(milliseconds.min(1000));
        self.process_samples = self.process_samples.wrapping_add(1);
    }

    pub fn take_process_average(&mut self) -> u16 {
        //647767..6477F4 consumes the accumulated average even if OutList is full.
        let average = self
            .process_milliseconds
            .checked_div(self.process_samples)
            .unwrap_or(0);
        self.process_milliseconds = 0;
        self.process_samples = 0;
        average as u16
    }

    pub fn take_precalculated(&mut self, house_id: i8, frame: i32) -> Option<TimingRecord> {
        //647E7B..647F30 clears both PreCalc globals after one append attempt.
        let (max_ahead, frames_per_second) = self.precalculated.take()?;
        Some(TimingRecord {
            house_id,
            frame,
            max_ahead,
            frames_per_second,
            frame_send_rate: NonZeroU8::new(if frames_per_second > 30 { 10 } else { 5 }).unwrap(),
        })
    }
}

fn scaled_response(response: u32, latency_fudge: i32) -> u32 {
    //6476DC jump table64831C: 0/other unchanged,1 adds unsigned half,2 doubles,3 triples.
    match latency_fudge {
        1 => response.wrapping_add(response >> 1),
        2 => response.wrapping_mul(2),
        3 => response.wrapping_mul(3),
        _ => response,
    }
}

pub(super) fn periodic_retry(response: u32, latency_fudge: i32) -> RetryTiming {
    let response = scaled_response(response, latency_fudge);
    //647700 uses unsigned JNC for this timeout floor.
    RetryTiming {
        retry_delta: response.wrapping_add(10),
        max_retries: u32::MAX,
        timeout: response.wrapping_mul(8).wrapping_add(15).max(120),
    }
}

/// ProcessNetwork647BF4..647E6D uses the slowest admitted PROCESS_TIME,
/// current game speed and private-channel response history. These are only
/// proposal inputs; applying the result belongs to EventClass on every peer.
pub(super) fn adaptive_timing(
    house_id: i8,
    frame: i32,
    mode: LiveCommandMode,
    process_milliseconds: u16,
    game_speed: i32,
    current_ahead: i32,
    current_rate: NonZeroU8,
    response: u32,
    retry_response: u32,
    latency_fudge: i32,
    fog_of_war: bool,
) -> (TimingRecord, RetryTiming) {
    let speed_budget = match game_speed {
        0 => 60,
        1 => 45,
        speed => 60 / speed,
    };
    let fps = if process_milliseconds == 0 {
        60
    } else {
        (1000 / i32::from(process_milliseconds)).max(1)
    }
    .min(speed_budget.max(1));
    let rate = if mode == LiveCommandMode::Wol {
        NonZeroU8::new(if fps > 30 { 10 } else { 5 }).unwrap()
    } else {
        current_rate
    };
    let extra = if response != 0 && (1..=3).contains(&latency_fudge) {
        latency_fudge * 10
    } else {
        0
    };
    let scaled = scaled_response(response, latency_fudge);
    let mut ahead = (scaled.wrapping_mul(fps as u32) / 120).wrapping_add(extra as u32) as i32;
    let rate_i32 = i32::from(rate.get());
    if ahead > current_ahead {
        ahead = ahead.min(current_ahead.wrapping_add(rate_i32));
    }
    ahead = ahead
        .wrapping_add(rate_i32 - 1)
        .wrapping_div(rate_i32)
        .wrapping_mul(rate_i32);
    ahead = ahead
        .max(rate_i32 * 3)
        .min((rate_i32 + 249) / rate_i32 * rate_i32);
    //647D24 uses signed truncation for the second response, independently of
    //the unsigned first response.647E56 uses a signed timeout comparison.
    let mut retry = retry_response as i32;
    if response != 0 {
        retry = match latency_fudge {
            1 => retry.wrapping_add(retry / 2),
            2 => retry.wrapping_mul(2),
            3 => retry.wrapping_mul(3),
            _ => retry,
        };
    }
    (
        TimingRecord {
            house_id,
            frame,
            frames_per_second: fps as u16,
            max_ahead: ahead.wrapping_add(if fog_of_war { 10 } else { 0 }) as u16,
            frame_send_rate: rate,
        },
        RetryTiming {
            retry_delta: retry.wrapping_add(10) as u32,
            max_retries: u32::MAX,
            timeout: retry.wrapping_mul(8).wrapping_add(15).max(120) as u32,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outcome_modal_keeps_one_processing_sample_until_queue_ai() {
        let mut timing = LiveTiming::default();
        timing.begin_processing(100);
        // A House modal suspends before Queue_AI, so no finish happens until
        // it resumes. Native55DE19 clamps that entire visit to1000ms.
        timing.finish_processing(5000);
        timing.finish_processing(6000); // Later network-wait polls are not Logic.
        assert_eq!(timing.take_process_average(), 1000);
        assert_eq!(timing.take_process_average(), 0);
        timing.begin_processing(u32::MAX - 10);
        timing.finish_processing(9);
        assert_eq!(timing.take_process_average(), 20);
    }
}
