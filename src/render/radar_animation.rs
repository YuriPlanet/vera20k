//! Ordinary tactical-radar housing state (movie/jammed modes are separate).
//!
//! Native `652960` starts offline at frame 0 with a zero-duration timer.
//! `656BE0` changes direction without resetting that timer. `653100` advances
//! once per draw when four `timeGetTime() >> 4` buckets have elapsed, resets
//! from the current bucket, and clamps at frames 0/32 in that same draw.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RadarAnimPhase {
    Offline,
    Opening,
    Online,
    Closing,
}

#[derive(Debug, Clone)]
pub(super) struct RadarAnimation {
    pub phase: RadarAnimPhase,
    pub frame: usize,
    last_bucket: i32,
    duration: i32,
}

impl Default for RadarAnimation {
    fn default() -> Self {
        Self {
            phase: RadarAnimPhase::Offline,
            frame: 0,
            last_bucket: 0,
            duration: 0,
        }
    }
}

impl RadarAnimation {
    pub fn set_has_radar(&mut self, available: bool) {
        match (available, self.phase) {
            (true, RadarAnimPhase::Offline | RadarAnimPhase::Closing) => {
                self.phase = RadarAnimPhase::Opening
            }
            (false, RadarAnimPhase::Online | RadarAnimPhase::Opening) => {
                self.phase = RadarAnimPhase::Closing
            }
            _ => {}
        }
    }

    pub fn advance_draw(&mut self, wall_ms: u64) -> bool {
        if matches!(self.phase, RadarAnimPhase::Offline | RadarAnimPhase::Online) {
            return false;
        }
        let bucket = ((wall_ms as u32) >> 4) as i32;
        // Native stopped timer sentinel keeps its remaining duration.
        let due = if self.last_bucket == -1 {
            self.duration == 0
        } else {
            bucket.wrapping_sub(self.last_bucket) >= self.duration
        };
        if !due {
            return false;
        }
        self.last_bucket = bucket;
        self.duration = 4;
        let old_frame = self.frame;
        match self.phase {
            RadarAnimPhase::Opening => {
                self.frame += 1;
                if self.frame >= 32 {
                    self.frame = 32;
                    self.phase = RadarAnimPhase::Online;
                }
            }
            RadarAnimPhase::Closing => {
                self.frame = self.frame.saturating_sub(1);
                if self.frame == 0 {
                    self.phase = RadarAnimPhase::Offline;
                }
            }
            _ => {}
        }
        self.frame != old_frame
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    #[derive(Deserialize)]
    struct Packet {
        cases: Vec<Case>,
    }
    #[derive(Deserialize)]
    struct Case {
        input: [i64; 5],
        output: [i64; 5],
    }
    fn phase(n: i64) -> RadarAnimPhase {
        match n {
            0 => RadarAnimPhase::Offline,
            1 => RadarAnimPhase::Online,
            2 => RadarAnimPhase::Closing,
            3 => RadarAnimPhase::Opening,
            _ => panic!("bad phase"),
        }
    }
    #[test]
    fn original_600_draw_timer_states_match() {
        let packet: Packet = serde_json::from_str(crate::test_fixture::text(
            "tools/sidebar_oracle/radar_timer.json",
        ))
        .unwrap();
        assert_eq!(packet.cases.len(), 600);
        for c in packet.cases {
            let mut a = RadarAnimation {
                phase: phase(c.input[0]),
                frame: c.input[1] as usize,
                last_bucket: c.input[2] as i32,
                duration: c.input[3] as i32,
            };
            a.advance_draw(c.input[4] as u64);
            assert_eq!(
                (a.phase, a.frame, a.last_bucket, a.duration),
                (
                    phase(c.output[0]),
                    c.output[1] as usize,
                    c.output[2] as i32,
                    c.output[3] as i32
                ),
                "{:?}",
                c.input
            );
        }
    }
    #[test]
    fn availability_reversal_keeps_draw_timer_and_stalls_do_not_catch_up() {
        let mut a = RadarAnimation::default();
        a.set_has_radar(true);
        assert!(a.advance_draw(1000));
        assert_eq!(a.frame, 1);
        a.set_has_radar(false);
        assert!(!a.advance_draw(1016));
        assert!(a.advance_draw(1056));
        assert_eq!((a.frame, a.phase), (0, RadarAnimPhase::Offline));
        a.set_has_radar(true);
        assert!(!a.advance_draw(1072));
        assert!(a.advance_draw(1_000_000));
        assert_eq!(a.frame, 1);
    }
}
