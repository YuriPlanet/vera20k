//! Link measurement shared by both ends of a hop (spec §4, §5): the receive
//! window, sender-side loss judgement, the client's RTT estimator and the
//! direction state clock. Every time is the caller's monotonic `now_ms`.

use super::{LinkState, RecvWindow};

/// Packets past the newest feedback left for the next one (spec §4.3).
pub(crate) const REORDER_THRESHOLD: u64 = 3;
/// Judged packets per fold of the smoothed loss (spec §4.3).
const LOSS_FOLD_PACKETS: u32 = 32;
const LOSSY_ENTER: u16 = 5;
const LOSSY_EXIT: u16 = 2;
/// A measured lossy state leaves only this long after its last loss.
const LOSSY_EXIT_QUIET_MS: u64 = 20_000;
const SEVERE_ENTER: u16 = 150;
const SEVERE_EXIT: u16 = 100;
const QUEUE_DELAY_FLOOR: u32 = 100;
const DELAY_MIN_SAMPLES: u64 = 8;
const BASE_WINDOW_MS: u64 = 10_000;
const RTO_INITIAL: u64 = 100;
const RTO_MIN: u64 = 48;
const RTO_MAX: u64 = 1000;
const PTO_MIN: u64 = 50;
const PTO_MAX: u64 = 1000;
/// `srtt_c` the relay assumes until a reader reports one (spec §4.6).
const PTO_DEFAULT_SRTT: u64 = 100;
const JITTER_MIN_SAMPLES: u64 = 16;
/// The JITTERY floor in µs (spec §4.8, `max(10, turn_ms / 4)` ms).
const JITTERY_FLOOR_US: u64 = 10_000;
/// srtt values above this share the last percentile bucket.
const SRTT_BUCKETS: usize = 2001;

impl RecvWindow {
    /// Counts authenticated hop sequence `sequence` (spec §4.1). Returns
    /// whether it became the new `high`.
    pub(crate) fn record(&mut self, sequence: u64) -> bool {
        if sequence > self.high {
            let advance = sequence - self.high;
            self.bits = if advance >= 64 {
                1
            } else {
                (self.bits << advance) | 1
            };
            self.high = sequence;
            return true;
        }
        let behind = self.high - sequence;
        if behind < 64 {
            self.bits |= 1 << behind;
        }
        false
    }
}

/// The sender's per-hop judgement cursor (spec §4.3 steps 1-3): it starts
/// again on a new hop (§4.7).
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct LossCursor {
    max_high: u64,
    eval_next: u64,
}

/// Smoothed loss of one direction and its hysteresis bands (spec §4.3 step
/// 4, §5.2, §5.4), with the statistics of §10.1.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct LossStats {
    judged: u32,
    lost: u32,
    pub(crate) permille: u16,
    pub(crate) folds: u64,
    pub(crate) permille_sum: u64,
    pub(crate) permille_max: u16,
    pub(crate) lossy: bool,
    pub(crate) severe: bool,
    // When the last lost packet was judged (spec §5.4).
    last_loss_ms: Option<u64>,
}

impl LossCursor {
    /// Judges feedback `(high, bits)` against what this hop sent (spec §4.3).
    /// `sent(s)` is the send time of hop sequence `s` if it was sent and is
    /// still logged; `lost(s)` is called for each one judged lost. A missing
    /// sequence sent less than `wait_ms` before `now_ms` stops the
    /// judgement there until the next feedback. Returns the judged and lost
    /// counts.
    pub(crate) fn judge(
        &mut self,
        high: u64,
        bits: u64,
        now_ms: u64,
        wait_ms: u64,
        mut sent: impl FnMut(u64) -> Option<u64>,
        mut lost: impl FnMut(u64),
    ) -> (u32, u32) {
        if high == 0 || high < self.max_high {
            return (0, 0);
        }
        self.max_high = high;
        let (mut judged, mut missing) = (0, 0);
        if high > REORDER_THRESHOLD {
            // Older sequences are outside the bitmap and never counted.
            let first = self.eval_next.max(high.saturating_sub(63)).max(1);
            for sequence in first..=high - REORDER_THRESHOLD {
                let Some(at) = sent(sequence) else {
                    continue;
                };
                let received = (bits >> (high - sequence)) & 1 == 1;
                if !received && now_ms.saturating_sub(at) < wait_ms {
                    // Maybe only reordered: judge it at a later feedback,
                    // unless it leaves the bitmap first.
                    self.eval_next = sequence;
                    return (judged, missing);
                }
                judged += 1;
                if !received {
                    missing += 1;
                    lost(sequence);
                }
            }
        }
        self.eval_next = self
            .eval_next
            .max(high.saturating_sub(REORDER_THRESHOLD - 1));
        (judged, missing)
    }
}

impl LossCursor {
    /// Judges nothing before hop sequence `next` (spec §5.5, on recovery).
    pub(crate) fn skip_to(&mut self, next: u64) {
        self.eval_next = self.eval_next.max(next);
    }
}

impl LossStats {
    /// A direction's loss before its first fold: treated as lossy (spec
    /// §5.4), so a new link has redundancy until it is measured.
    pub(crate) fn initial() -> Self {
        Self {
            lossy: true,
            ..Self::default()
        }
    }

    /// Test support: one fold done at 0‰, so the direction is good and
    /// later folds keep the hysteresis.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn measured_good() -> Self {
        Self {
            folds: 1,
            ..Self::default()
        }
    }

    /// Adds one judgement's counts and folds once 32 are judged (spec §4.3
    /// step 4), then applies the lossy and severe bands (spec §5.2, §5.4).
    /// `now_ms` is when the counts were judged.
    pub(crate) fn add(&mut self, judged: u32, lost: u32, now_ms: u64) {
        self.judged += judged;
        self.lost += lost;
        if lost > 0 {
            self.last_loss_ms = Some(now_ms);
        }
        if self.judged < LOSS_FOLD_PACKETS {
            return;
        }
        let sample = (u64::from(self.lost) * 1000 / u64::from(self.judged)) as u32;
        self.permille = ((3 * u32::from(self.permille) + sample) / 4) as u16;
        self.judged = 0;
        self.lost = 0;
        self.folds += 1;
        self.permille_sum += u64::from(self.permille);
        self.permille_max = self.permille_max.max(self.permille);
        // The first fold decides afresh: the initial lossy state was no
        // measurement (spec §5.4). Later folds keep the hysteresis.
        let first = self.folds == 1;
        self.lossy = if self.lossy && !first {
            // Leaving also takes 20 s without any lost packet.
            self.permille > LOSSY_EXIT
                || self
                    .last_loss_ms
                    .is_some_and(|at| now_ms.saturating_sub(at) < LOSSY_EXIT_QUIET_MS)
        } else {
            self.permille >= LOSSY_ENTER
        };
        self.severe = if self.severe && !first {
            self.permille > SEVERE_EXIT
        } else {
            self.permille >= SEVERE_ENTER
        };
    }

    /// Entering the offline state discards the open counts (spec §5.5).
    pub(crate) fn clear_counts(&mut self) {
        self.judged = 0;
        self.lost = 0;
    }
}

/// The client's RTT estimate (spec §4.4, §4.5, §5.3): RFC 6298 smoothing,
/// the 10 s windowed base RTT and the DELAYED judgement.
#[derive(Debug, Clone)]
pub(crate) struct RttEstimator {
    pub(crate) srtt: Option<u32>,
    pub(crate) rttvar: u32,
    pub(crate) samples: u64,
    window_previous: Option<u32>,
    window_current: Option<u32>,
    window_start: u64,
    pub(crate) delayed: bool,
    // srtt after each sample, in 1 ms buckets, for its percentiles.
    histogram: Vec<u32>,
}

impl Default for RttEstimator {
    fn default() -> Self {
        Self {
            srtt: None,
            rttvar: 0,
            samples: 0,
            window_previous: None,
            window_current: None,
            window_start: 0,
            delayed: false,
            histogram: vec![0; SRTT_BUCKETS],
        }
    }
}

impl RttEstimator {
    /// One RTT sample `rtt` ms taken at `now_ms` (spec §4.4 steps 4-5, §5.3).
    pub(crate) fn sample(&mut self, rtt: u32, now_ms: u64) {
        match self.srtt {
            None => {
                self.srtt = Some(rtt);
                self.rttvar = rtt / 2;
            }
            Some(srtt) => {
                self.rttvar = (3 * self.rttvar + srtt.abs_diff(rtt)) / 4;
                self.srtt = Some(((7 * u64::from(srtt) + u64::from(rtt)) / 8) as u32);
            }
        }
        if self.samples == 0 {
            self.window_start = now_ms;
        } else if now_ms.saturating_sub(self.window_start) >= BASE_WINDOW_MS {
            self.window_previous = self.window_current.or(self.window_previous);
            self.window_current = None;
            self.window_start = now_ms;
        }
        self.window_current = Some(self.window_current.map_or(rtt, |low| low.min(rtt)));
        self.samples += 1;
        let srtt = self.srtt.expect("sampled");
        self.histogram[(srtt as usize).min(SRTT_BUCKETS - 1)] += 1;
        if self.samples >= DELAY_MIN_SAMPLES {
            let base = self.base().expect("sampled");
            let queue = base.max(QUEUE_DELAY_FLOOR);
            // Jitter is no queueing delay (spec §5.3).
            let steady = srtt.saturating_sub(2 * self.rttvar);
            if self.delayed {
                self.delayed = steady > base + queue / 2;
            } else {
                self.delayed = steady >= base + queue;
            }
        }
    }

    /// `min(win_prev, win_cur)` (spec §4.4 step 5).
    pub(crate) fn base(&self) -> Option<u32> {
        match (self.window_previous, self.window_current) {
            (Some(previous), Some(current)) => Some(previous.min(current)),
            (previous, current) => previous.or(current),
        }
    }

    /// `RTO_k` in ms (spec §4.5).
    pub(crate) fn rto(&self, backoff: u32) -> u64 {
        let rto = match self.srtt {
            None => RTO_INITIAL,
            Some(srtt) => {
                (u64::from(srtt) + 16.max(4 * u64::from(self.rttvar))).clamp(RTO_MIN, RTO_MAX)
            }
        };
        backed_off(rto, backoff, RTO_MAX)
    }

    /// The `percent` percentile of srtt after each sample.
    pub(crate) fn srtt_percentile(&self, percent: u64) -> Option<u32> {
        if self.samples == 0 {
            return None;
        }
        let rank = (self.samples * percent).div_ceil(100).max(1);
        let mut seen = 0;
        for (value, count) in self.histogram.iter().enumerate() {
            seen += u64::from(*count);
            if seen >= rank {
                return Some(value as u32);
            }
        }
        None
    }
}

fn backed_off(timeout: u64, backoff: u32, max: u64) -> u64 {
    timeout
        .checked_shl(backoff)
        .filter(|value| value >> backoff == timeout)
        .unwrap_or(max)
        .min(max)
}

/// The client's downlink arrival jitter of new-turn packs (spec §4.8):
/// RFC 3550 smoothing in µs and the JITTERY hysteresis, with the time it
/// held and how often it was entered (spec §10.1).
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct JitterMeter {
    // The previous new-turn pack: its arrival ms and newest frame.
    previous: Option<(u64, u32)>,
    pub(crate) jitter_us: u64,
    pub(crate) max_us: u64,
    samples: u64,
    pub(crate) jittery: bool,
    since: Option<u64>,
    pub(crate) jittery_ms: u64,
    pub(crate) entries: u64,
}

impl JitterMeter {
    /// A new-turn pack whose newest frame is `frame` arrived at `now_ms`,
    /// sealed at `rate` frames a second, with turns of `turn_ms`.
    pub(crate) fn sample(&mut self, now_ms: u64, frame: u32, rate: u16, turn_ms: u32) {
        if let Some((at, previous_frame)) = self.previous {
            let arrived_us = i128::from(now_ms.saturating_sub(at)) * 1000;
            let sealed_us = i128::from(frame.saturating_sub(previous_frame)) * 1_000_000
                / i128::from(rate.max(1));
            let deviation = (arrived_us - sealed_us).unsigned_abs() as u64;
            // J = J + (|D| - J) / 16, in signed steps.
            let jitter = self.jitter_us as i128;
            self.jitter_us = (jitter + (i128::from(deviation) - jitter) / 16).max(0) as u64;
            self.max_us = self.max_us.max(self.jitter_us);
            self.samples += 1;
            if self.samples >= JITTER_MIN_SAMPLES {
                let enter = JITTERY_FLOOR_US.max(u64::from(turn_ms) * 1000 / 4);
                let jittery = if self.jittery {
                    self.jitter_us > enter / 2
                } else {
                    self.jitter_us >= enter
                };
                self.set(jittery, now_ms);
            }
        }
        self.previous = Some((now_ms, frame));
    }

    fn set(&mut self, jittery: bool, now_ms: u64) {
        match (self.jittery, jittery) {
            (false, true) => {
                self.entries += 1;
                self.since = Some(now_ms);
            }
            (true, false) => {
                self.jittery_ms += self.since.map_or(0, |at| now_ms.saturating_sub(at));
                self.since = None;
            }
            _ => {}
        }
        self.jittery = jittery;
    }

    /// The next new-turn pack only sets the reference again: after a
    /// pause, an outage or a PULL (spec §4.8).
    pub(crate) fn restart(&mut self) {
        self.previous = None;
    }

    /// JITTERY's total time including the open span.
    pub(crate) fn jittery_total(&self, now_ms: u64) -> u64 {
        self.jittery_ms + self.since.map_or(0, |at| now_ms.saturating_sub(at))
    }
}

/// The reader's reported `(srtt_c, rttvar_c)` in ms, or the §4.6 defaults
/// 100 / 50 while it reports no srtt.
pub(crate) fn reader_rtt(srtt_c: u16, rttvar_c: u16) -> (u64, u64) {
    if srtt_c == 0 {
        (PTO_DEFAULT_SRTT, PTO_DEFAULT_SRTT / 2)
    } else {
        (u64::from(srtt_c), u64::from(rttvar_c))
    }
}

/// `PTO_k` in ms (spec §4.6) from the reader's reported srtt and rttvar
/// (0 for none); `ack_delay_max` is its state's acknowledgement interval.
pub(crate) fn pto(srtt_c: u16, rttvar_c: u16, ack_delay_max: u64, backoff: u32) -> u64 {
    let (srtt, rttvar) = reader_rtt(srtt_c, rttvar_c);
    let pto = (srtt + 16.max(4 * rttvar) + ack_delay_max).clamp(PTO_MIN, PTO_MAX);
    backed_off(pto, backoff, PTO_MAX)
}

/// One direction's state (spec §5.2) with the time spent in and entries
/// into each, and the time DELAYED held (spec §10.1).
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct StateClock {
    pub(crate) state: LinkState,
    since: Option<u64>,
    pub(crate) state_ms: [u64; 4],
    pub(crate) entries: [u64; 4],
    delayed_since: Option<u64>,
    pub(crate) delayed_ms: u64,
    // The latest `enter` time.
    at: u64,
}

impl StateClock {
    /// Moves to `state` at `now_ms`; returns the previous state if it changed.
    pub(crate) fn enter(
        &mut self,
        state: LinkState,
        delayed: bool,
        now_ms: u64,
    ) -> Option<LinkState> {
        let since = *self.since.get_or_insert(now_ms);
        self.at = self.at.max(now_ms);
        match (self.delayed_since, delayed) {
            (None, true) => self.delayed_since = Some(now_ms),
            (Some(start), false) => {
                self.delayed_ms += now_ms.saturating_sub(start);
                self.delayed_since = None;
            }
            _ => {}
        }
        if state == self.state {
            return None;
        }
        self.state_ms[self.state.index()] += now_ms.saturating_sub(since);
        self.since = Some(now_ms);
        self.entries[state.index()] += 1;
        Some(std::mem::replace(&mut self.state, state))
    }

    /// The per-state durations and DELAYED time including the open spans.
    pub(crate) fn totals(&self, now_ms: u64) -> ([u64; 4], u64) {
        let mut state_ms = self.state_ms;
        if let Some(since) = self.since {
            state_ms[self.state.index()] += now_ms.saturating_sub(since);
        }
        let delayed = self.delayed_ms
            + self
                .delayed_since
                .map_or(0, |start| now_ms.saturating_sub(start));
        (state_ms, delayed)
    }

    /// The latest evaluation time, for snapshots.
    pub(crate) fn last_ms(&self) -> u64 {
        self.at
    }
}

/// The spec §5.2 order: offline, congested, lossy, good.
pub(crate) fn link_state(offline: bool, congested: bool, lossy: bool) -> LinkState {
    if offline {
        LinkState::Offline
    } else if congested {
        LinkState::Congested
    } else if lossy {
        LinkState::Lossy
    } else {
        LinkState::Good
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_receive_window_counts_ahead_reordered_far_jumps_and_ignores_old() {
        let mut window = RecvWindow::default();
        assert!(window.record(1));
        assert!(window.record(3));
        assert_eq!((window.high, window.bits), (3, 0b101));
        // Reordered: the missing 2 fills its bit.
        assert!(!window.record(2));
        assert_eq!(window.bits, 0b111);
        // Exactly 64 ahead restarts the bitmap.
        window.record(67);
        assert_eq!((window.high, window.bits), (67, 1));
        window.record(70);
        // 63 behind is the oldest bit; 64 behind changes nothing.
        window.record(7);
        assert_eq!(window.bits, 0b1001 | 1 << 63);
        window.record(6);
        assert_eq!(window.bits, 0b1001 | 1 << 63);
    }

    #[test]
    fn loss_is_judged_three_behind_the_newest_feedback_inside_the_bitmap_only() {
        let mut cursor = LossCursor::default();
        let mut lost = Vec::new();
        // 1..=10 sent; 7 and 8 missing. Up to 10 - 3 = 7 is judged now.
        let bits = 0b11_1111_1111 & !(1 << 3) & !(1 << 2);
        assert_eq!(
            cursor.judge(10, bits, 1000, 0, |_| Some(0), |s| lost.push(s)),
            (7, 1)
        );
        assert_eq!(lost, [7]);
        // An older feedback arriving late is skipped.
        assert_eq!(
            cursor.judge(9, 0, 1000, 0, |_| Some(0), |s| lost.push(s)),
            (0, 0)
        );
        // 8..=10 are judged at the next feedback; 8 is still missing.
        assert_eq!(
            cursor.judge(13, !(1 << 5), 1000, 0, |_| Some(0), |s| lost.push(s)),
            (3, 1)
        );
        assert_eq!(lost, [7, 8]);
        // Beyond the 64-bit map: only 137..=197 can be judged from 200.
        let mut far = LossCursor::default();
        assert_eq!(
            far.judge(200, u64::MAX, 1000, 0, |_| Some(0), |_| {}),
            (61, 0)
        );
        // Unsent sequences (HELLO) are not judged.
        let mut cursor = LossCursor::default();
        assert_eq!(
            cursor.judge(8, 0x7F, 1000, 0, |s| (s >= 2).then_some(0), |_| {}),
            (4, 0)
        );
    }

    #[test]
    fn a_recovered_link_judges_nothing_it_sent_before() {
        let mut cursor = LossCursor::default();
        // Offline through 20; the recovery's next send is 21.
        cursor.skip_to(21);
        // Only 24 arrived of 1..=24, yet nothing up to 20 counts.
        assert_eq!(cursor.judge(24, 1, 1000, 0, |_| Some(0), |_| {}), (1, 1));
    }

    #[test]
    fn loss_folds_every_32_judged_packets_with_hysteresis() {
        let mut stats = LossStats::default();
        stats.add(31, 0, 0);
        assert_eq!(stats.folds, 0);
        stats.add(1, 0, 0);
        assert_eq!((stats.permille, stats.folds, stats.lossy), (0, 1, false));
        // 1 of 32: sample 31, (0 + 31) / 4 = 7, at least the 5‰ entry.
        stats.add(32, 1, 0);
        assert_eq!((stats.permille, stats.lossy), (7, true));
        // 5, 3, 2: at the 2‰ exit, but a loss within 20 s keeps it lossy.
        for _ in 0..3 {
            stats.add(32, 0, 19_999);
        }
        assert_eq!((stats.permille, stats.lossy), (2, true));
        stats.add(32, 0, 20_000);
        assert!(!stats.lossy, "20 s after the last loss, at most 2‰");
        stats.add(32, 32, 0); // (0 + 1000) / 4 = 250
        assert_eq!((stats.permille, stats.severe), (250, true));
        stats.add(32, 0, 0); // 187
        stats.add(32, 0, 0); // 140
        stats.add(32, 0, 0); // 105
        assert!(stats.severe);
        stats.add(32, 0, 0); // 78
        assert!(!stats.severe);
        assert_eq!(stats.permille_max, 250);
        // Another loss within the 20 s restarts the wait.
        let mut stats = LossStats::initial();
        stats.add(32, 1, 0); // first fold: 7, lossy
        stats.add(320, 1, 15_000); // another loss at 15 s
        for _ in 0..5 {
            stats.add(32, 0, 34_999);
        }
        assert_eq!((stats.permille, stats.lossy), (0, true));
        stats.add(32, 0, 35_000);
        assert!(!stats.lossy);
    }

    #[test]
    fn jitter_smooths_new_turn_deviations_and_needs_16_samples() {
        let mut meter = JitterMeter::default();
        // 30 Hz, two-frame turns 66.7 ms apart; every other turn 60 ms late.
        let mut at = 0;
        for turn in 0..=16u32 {
            at += if turn % 2 == 0 { 67 } else { 7 + 60 + 60 };
            meter.sample(at, 2 * turn, 30, 67);
        }
        assert!(meter.jitter_us > 16_750, "{}", meter.jitter_us);
        assert!(meter.jittery, "16 samples: enter at max(10, 67/4) ms");
        assert_eq!(meter.entries, 1);
        // On time again: J decays and JITTERY clears at half the entry.
        for turn in 17..200u32 {
            at += 67;
            meter.sample(at, 2 * turn, 30, 67);
        }
        assert!(!meter.jittery);
        assert!(meter.jittery_total(at) > 0);
        // A restart takes no sample from the gap.
        meter.restart();
        let before = meter.jitter_us;
        meter.sample(at + 5000, 400, 30, 67);
        assert_eq!(meter.jitter_us, before);
    }

    #[test]
    fn rtt_smooths_rolls_its_base_window_and_judges_queueing_delay() {
        let mut rtt = RttEstimator::default();
        assert_eq!(rtt.rto(0), 100);
        rtt.sample(40, 0);
        assert_eq!((rtt.srtt, rtt.rttvar, rtt.base()), (Some(40), 20, Some(40)));
        rtt.sample(40, 100);
        assert_eq!((rtt.srtt, rtt.rttvar), (Some(40), 15));
        assert_eq!(rtt.rto(0), 100);
        assert_eq!(rtt.rto(4), 1000);
        // A new window keeps the previous minimum until it rolls again.
        rtt.sample(60, 10_000);
        assert_eq!(rtt.base(), Some(40));
        rtt.sample(60, 20_000);
        assert_eq!(rtt.base(), Some(60));
        // Eight samples, then srtt - 2 rttvar >= base + max(base, 100) sets
        // DELAYED.
        for at in 0..40 {
            rtt.sample(400, 20_100 + at);
        }
        assert!(rtt.delayed);
        for at in 0..60 {
            rtt.sample(60, 21_000 + at);
        }
        assert!(!rtt.delayed);
        assert_eq!(rtt.srtt_percentile(0), Some(40));
    }

    #[test]
    fn the_initial_lossy_state_turns_good_at_a_clean_first_fold() {
        // Spec §5.4: the eight clean folds apply only to a measured loss.
        let mut stats = LossStats::initial();
        assert!(stats.lossy);
        stats.add(32, 0, 0);
        assert_eq!((stats.folds, stats.permille, stats.lossy), (1, 0, false));
    }

    #[test]
    fn jitter_around_a_doubled_srtt_is_no_queueing_delay() {
        // ±50 ms around 200 ms each way: srtt about twice a base of ~100.
        let mut rtt = RttEstimator::default();
        for at in 0..200u64 {
            rtt.sample(if at % 2 == 0 { 100 } else { 300 }, at * 50);
        }
        assert!(rtt.srtt.unwrap() >= rtt.base().unwrap() + 100);
        assert!(!rtt.delayed, "srtt {:?} rttvar {}", rtt.srtt, rtt.rttvar);
    }

    #[test]
    fn a_missing_packet_waits_an_rtt_before_it_counts_as_lost() {
        let mut cursor = LossCursor::default();
        let mut lost = Vec::new();
        // 1..=10 sent at 100 + s; 5 missing. The feedback arrives at 150,
        // 45 ms after it went: under the 100 ms wait, judging stops there.
        let bits = 0b11_1111_1111 & !(1 << 5);
        let sent = |s: u64| Some(100 + s);
        assert_eq!(
            cursor.judge(10, bits, 150, 100, sent, |s| lost.push(s)),
            (4, 0)
        );
        // The next feedback arrives 105 ms after it went: lost.
        assert_eq!(
            cursor.judge(11, bits << 1 | 1, 210, 100, sent, |s| lost.push(s)),
            (4, 1)
        );
        assert_eq!(lost, [5]);
    }

    #[test]
    fn appendix_a_rtt_samples_give_rttvar_11() {
        // Three 40 ms samples: rttvar 20, 15, 11.25 -> 11 (spec appendix A).
        let mut rtt = RttEstimator::default();
        for at in [0, 100, 200] {
            rtt.sample(40, at);
        }
        assert_eq!((rtt.srtt, rtt.rttvar), (Some(40), 11));
    }

    #[test]
    fn pto_defaults_and_backs_off_to_its_cap() {
        // srtt_c 100, rttvar_c 50: 100 + 200 + 100.
        assert_eq!(pto(0, 0, 100, 0), 400);
        // The reported rttvar_c: 20 + max(16, 4 * 3) + 100.
        assert_eq!(pto(20, 3, 100, 0), 136);
        assert_eq!(pto(20, 10, 100, 0), 160);
        assert_eq!(pto(1, 0, 0, 0), 50);
        assert_eq!(pto(20, 10, 100, 3), 1000);
    }

    #[test]
    fn the_state_clock_counts_entries_and_time_per_state() {
        let mut clock = StateClock::default();
        assert_eq!(clock.enter(LinkState::Good, false, 0), None);
        assert_eq!(
            clock.enter(LinkState::Lossy, true, 100),
            Some(LinkState::Good)
        );
        assert_eq!(
            clock.enter(LinkState::Offline, false, 350),
            Some(LinkState::Lossy)
        );
        let (state_ms, delayed) = clock.totals(400);
        assert_eq!(state_ms, [100, 250, 0, 50]);
        assert_eq!(clock.entries, [0, 1, 0, 1]);
        assert_eq!(delayed, 250);
        assert_eq!(link_state(true, true, true), LinkState::Offline);
        assert_eq!(link_state(false, true, true), LinkState::Congested);
        assert_eq!(link_state(false, false, true), LinkState::Lossy);
    }
}
