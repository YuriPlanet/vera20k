//! Per-match frame-time accounting for the end-of-match report.
//!
//! The dashboard reports one tick cost and one frame rate, but a report that
//! answers "where did the time go" needs the frame split into phases. This
//! module is the accumulator; the phase boundaries themselves are stamped in
//! `Simulation::advance_master_frame`, next to the phase comments that already
//! document that function.
//!
//! ## One ledger, two readings
//! The frame is stamped at every subsystem boundary and kept as a flat array of
//! spans. The four coarse buckets the pies draw are *sums* over slices of that
//! same array, so a pie can never disagree with the table beside it.
//!
//! ## Discipline
//! - **Derived data only.** The accumulator lives on `Simulation` behind
//!   `#[serde(skip)]` and is never folded into the state hash or the retail
//!   multiplayer checksum: wall time is not a simulation fact.
//! - **Nanosecond integers.** Accumulation stays in `u128` so a long soak does
//!   not drift; conversion to milliseconds happens once, in [`FrameProfileReport`].
//! - **Opt-in.** New and deserialized simulations do no clock reads. Enabled
//!   measurement stamps existing boundaries without moving gameplay work.
//! - **Current boundaries.** Combat, passengers and wall crushing mostly run
//!   inside the live object pass. Slots without a separate current phase stay
//!   zero; they do not claim an independently measured subsystem.

use std::ops::Range;
use crate::util::time::{Duration, Instant};

/// A local, enabled-only observer; never held by or serialized with the world.
pub(crate) struct FrameClock {
    started: Instant,
    previous: Instant,
    phases: FramePhases,
}

impl FrameClock {
    pub(crate) fn start() -> Self {
        let now = Instant::now();
        Self {
            started: now,
            previous: now,
            phases: FramePhases::default(),
        }
    }

    pub(crate) fn stamp(&mut self, phase: usize) {
        let now = Instant::now();
        self.phases.add(phase, now.duration_since(self.previous));
        self.previous = now;
    }

    pub(crate) fn finish(self) -> (FramePhases, Duration) {
        (self.phases, self.previous.duration_since(self.started))
    }
}

/// Slots in the per-frame ledger, in the order `advance_master_frame` runs
/// them. Names are stable keys: the dashboard's i18n table is indexed by them.
pub mod phase {
    pub const COMMANDS_INGRESS: usize = 0;
    pub const COMMANDS_ADMIT: usize = 1;
    pub const TRIGGERS: usize = 2;
    pub const SHROUD: usize = 3;
    pub const LIGHTING: usize = 4;
    pub const ORE_GROWTH: usize = 5;
    pub const BOMBS: usize = 6;
    pub const SUPERWEAPONS: usize = 7;
    pub const RADIATION: usize = 8;
    pub const TEAM_AI: usize = 9;
    pub const OBJECTS: usize = 10;
    pub const WALL_CRUSH: usize = 11;
    pub const WAKES: usize = 12;
    pub const VISION: usize = 13;
    pub const COMBAT: usize = 14;
    pub const PASSENGERS: usize = 15;
    pub const ION_BLASTS: usize = 16;
    pub const CRATES: usize = 17;
    pub const PRODUCTION: usize = 18;
    pub const DOCKS: usize = 19;
    pub const SPAWN_FOG: usize = 20;
    pub const LATE_AI: usize = 21;
    pub const OVERLAYS: usize = 22;
    pub const DEBUG_ASSERTS: usize = 23;
    pub const ANIMS: usize = 24;
    pub const SCORE: usize = 25;
    pub const HASH: usize = 26;
}

/// Number of stamped phases per frame.
pub const PHASE_COUNT: usize = 27;

/// Wire keys for the ledger, indexed by the `phase::*` slots.
///
/// Order is the contract with the dashboard: `phases_ms[i]` means
/// `PHASE_KEYS[i]`. The dashboard holds the same list in `PHASE_KEYS`.
pub const PHASE_KEYS: [&str; PHASE_COUNT] = [
    "commands_ingress",
    "commands_admit",
    "triggers",
    "shroud",
    "lighting",
    "ore_growth",
    "bombs",
    "superweapons",
    "radiation",
    "team_ai",
    "objects",
    "wall_crush",
    "wakes",
    "vision",
    "combat",
    "passengers",
    "ion_blasts",
    "crates",
    "production",
    "docks",
    "spawn_fog",
    "late_ai",
    "overlays",
    "debug_asserts",
    "anims",
    "score",
    "hash",
];

/// Bucket slices. Contiguous by construction, so the pies and the table read
/// the same numbers.
const LOGIC_SLICE: Range<usize> = phase::COMMANDS_INGRESS..phase::OBJECTS;
const OBJECTS_SLICE: Range<usize> = phase::OBJECTS..phase::VISION;
const VISION_SLICE: Range<usize> = phase::VISION..phase::COMBAT;
const TAIL_SLICE: Range<usize> = phase::COMBAT..PHASE_COUNT;

/// One frame's phase spans, stamped at every subsystem boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FramePhases {
    spans: [Duration; PHASE_COUNT],
}

impl Default for FramePhases {
    fn default() -> Self {
        Self {
            spans: [Duration::ZERO; PHASE_COUNT],
        }
    }
}

impl FramePhases {
    /// Record one slot's span. A slot is stamped once per frame; a second stamp
    /// replaces the first rather than doubling it.
    pub fn set(&mut self, index: usize, span: Duration) {
        self.spans[index] = span;
    }

    /// Fold a span into a slot that runs in more than one stretch.
    pub fn add(&mut self, index: usize, span: Duration) {
        self.spans[index] += span;
    }

    /// One slot's span.
    pub fn get(&self, index: usize) -> Duration {
        self.spans[index]
    }

    /// Sum over a contiguous slice of the ledger.
    pub fn sum(&self, slice: Range<usize>) -> Duration {
        self.spans[slice].iter().copied().sum()
    }

    /// Everything before the live object pass — the `logic` pie wedge.
    pub fn logic(&self) -> Duration {
        self.sum(LOGIC_SLICE)
    }

    /// The live object pass and the post-pass steps that belong to it.
    pub fn objects(&self) -> Duration {
        self.sum(OBJECTS_SLICE)
    }

    /// Vision / fog refresh.
    pub fn vision(&self) -> Duration {
        self.sum(VISION_SLICE)
    }

    /// Everything after vision — the `tail` pie wedge.
    pub fn tail(&self) -> Duration {
        self.sum(TAIL_SLICE)
    }

    /// The whole simulation frame these phases claim.
    pub fn sim(&self) -> Duration {
        self.logic() + self.objects() + self.vision() + self.tail()
    }
}

/// Accumulated phase times for one match (or since the last reset).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FrameProfile {
    frames: u64,
    /// Per-phase totals, indexed by the `phase::*` slots.
    spans_ns: [u128; PHASE_COUNT],
    /// The measured span of the whole simulation frame, for the residual check.
    total_ns: u128,
    peak_frame_ns: u128,
}

impl FrameProfile {
    /// Fold one completed simulation frame into the totals.
    pub fn record(&mut self, phases: FramePhases, total: Duration) {
        self.frames += 1;
        for (slot, span) in self.spans_ns.iter_mut().zip(phases.spans) {
            *slot += span.as_nanos();
        }
        self.total_ns += total.as_nanos();
        self.peak_frame_ns = self.peak_frame_ns.max(total.as_nanos());
    }

    /// Start a fresh match's accounting.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Completed frame calls measured so far, including a terminal frame.
    pub fn frames(&self) -> u64 {
        self.frames
    }

    /// The simulation-side report. `None` before the first frame is measured.
    pub fn report(&self) -> Option<FrameProfileReport> {
        if self.frames == 0 {
            return None;
        }
        let frames = self.frames as f64;
        let mut phases_ms = [0.0f64; PHASE_COUNT];
        for (slot, ms) in phases_ms.iter_mut().zip(self.spans_ns) {
            *slot = ns_to_ms(ms) / frames;
        }
        // Derived from the same slots the table lists, never accumulated twice.
        let logic = self.spans_ns[LOGIC_SLICE].iter().sum::<u128>();
        let objects = self.spans_ns[OBJECTS_SLICE].iter().sum::<u128>();
        let vision = self.spans_ns[VISION_SLICE].iter().sum::<u128>();
        let tail = self.spans_ns[TAIL_SLICE].iter().sum::<u128>();
        let bucketed = logic + objects + vision + tail;
        // A frame can abort before its stamps land; the difference between the
        // measured span and the buckets is exactly that unaccounted time.
        let unmarked = self.total_ns.saturating_sub(bucketed);
        Some(FrameProfileReport {
            frames: self.frames,
            mean_frame_ms: ns_to_ms(self.total_ns) / frames,
            peak_frame_ms: ns_to_ms(self.peak_frame_ns),
            logic_ms: ns_to_ms(logic) / frames,
            objects_ms: ns_to_ms(objects) / frames,
            vision_ms: ns_to_ms(vision) / frames,
            tail_ms: ns_to_ms(tail) / frames,
            unmarked_ms: ns_to_ms(unmarked) / frames,
            phases_ms,
        })
    }
}

fn ns_to_ms(nanos: u128) -> f64 {
    nanos as f64 / 1.0e6
}

/// Mean (and peak) simulation frame time split by phase, in milliseconds.
///
/// App-filled on the snapshot: the wire pairs these buckets with the app's own
/// frame time so a report can show "simulation vs everything else".
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FrameProfileReport {
    /// Frames measured.
    pub frames: u64,
    /// Mean whole simulation frame, all phases.
    pub mean_frame_ms: f64,
    /// Slowest single simulation frame.
    pub peak_frame_ms: f64,
    /// Pre-object logic: commands, triggers, scripts, lighting, ore growth.
    pub logic_ms: f64,
    /// Live object pass: AI, movement, firing.
    pub objects_ms: f64,
    /// Vision / fog refresh.
    pub vision_ms: f64,
    /// Combat tail, passengers, production, defeat, commit.
    pub tail_ms: f64,
    /// Measured time the four buckets did not claim (aborted frames).
    pub unmarked_ms: f64,
    /// Per-phase means, indexed by [`PHASE_KEYS`].
    pub phases_ms: [f64; PHASE_COUNT],
}

impl Default for FrameProfileReport {
    fn default() -> Self {
        Self {
            frames: 0,
            mean_frame_ms: 0.0,
            peak_frame_ms: 0.0,
            logic_ms: 0.0,
            objects_ms: 0.0,
            vision_ms: 0.0,
            tail_ms: 0.0,
            unmarked_ms: 0.0,
            phases_ms: [0.0; PHASE_COUNT],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Spread `total` evenly over a slice, putting the division remainder on the
    /// slice's first slot so the sum stays exact.
    fn fill(phases: &mut FramePhases, slice: Range<usize>, total: Duration) {
        let count = (slice.end - slice.start) as u32;
        let each = total / count;
        for slot in slice.clone() {
            phases.set(slot, each);
        }
        phases.add(slice.start, total - each * count);
    }

    /// A frame whose spans are spread evenly over each bucket, so the bucket
    /// sums stay easy to read.
    fn spread(logic: Duration, objects: Duration, vision: Duration, tail: Duration) -> FramePhases {
        let mut phases = FramePhases::default();
        fill(&mut phases, LOGIC_SLICE, logic);
        fill(&mut phases, OBJECTS_SLICE, objects);
        phases.set(phase::VISION, vision);
        fill(&mut phases, TAIL_SLICE, tail);
        phases
    }

    #[test]
    fn buckets_average_over_frames() {
        let mut profile = FrameProfile::default();
        for _ in 0..4 {
            profile.record(
                spread(
                    Duration::from_micros(100),
                    Duration::from_micros(200),
                    Duration::from_micros(300),
                    Duration::from_micros(400),
                ),
                Duration::from_micros(1000),
            );
        }
        let report = profile.expect_report();
        assert_eq!(report.frames, 4);
        assert_eq!(report.mean_frame_ms, 1.0);
        assert_eq!(report.logic_ms, 0.1);
        assert_eq!(report.objects_ms, 0.2);
        assert_eq!(report.vision_ms, 0.3);
        assert_eq!(report.tail_ms, 0.4);
        assert_eq!(
            report.unmarked_ms, 0.0,
            "the buckets cover the frame exactly"
        );
    }

    #[test]
    fn sub_phases_sum_to_their_buckets() {
        let mut profile = FrameProfile::default();
        let mut phases = FramePhases::default();
        phases.set(phase::COMMANDS_INGRESS, Duration::from_micros(10));
        phases.set(phase::COMMANDS_ADMIT, Duration::from_micros(20));
        phases.set(phase::TRIGGERS, Duration::from_micros(30));
        phases.set(phase::TEAM_AI, Duration::from_micros(40));
        phases.set(phase::OBJECTS, Duration::from_micros(500));
        phases.set(phase::VISION, Duration::from_micros(100));
        phases.set(phase::COMBAT, Duration::from_micros(60));
        phases.set(phase::PRODUCTION, Duration::from_micros(50));
        phases.set(phase::LATE_AI, Duration::from_micros(40));
        phases.set(phase::HASH, Duration::from_micros(30));
        assert_eq!(phases.logic(), Duration::from_micros(100), "10+20+30+40 µs");
        assert_eq!(phases.tail(), Duration::from_micros(180), "60+50+40+30 µs");
        profile.record(phases, Duration::from_micros(880));
        let report = profile.expect_report();
        assert_eq!(report.logic_ms, 0.1);
        assert_eq!(report.tail_ms, 0.18);
        assert_eq!(report.phases_ms[phase::COMMANDS_INGRESS], 0.01);
        assert_eq!(report.phases_ms[phase::TEAM_AI], 0.04);
        assert_eq!(report.phases_ms[phase::HASH], 0.03);
        assert_eq!(report.unmarked_ms, 0.0);
    }

    #[test]
    fn every_slot_has_a_key_and_exactly_one_bucket_owns_it() {
        assert_eq!(PHASE_KEYS.len(), PHASE_COUNT);
        for (index, key) in PHASE_KEYS.iter().enumerate() {
            assert!(!key.is_empty(), "slot {index} has no wire key");
            assert!(
                key.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
                "slot {index} key `{key}` is not a stable identifier"
            );
            let buckets = [&LOGIC_SLICE, &OBJECTS_SLICE, &VISION_SLICE, &TAIL_SLICE]
                .iter()
                .filter(|slice| slice.contains(&index))
                .count();
            assert_eq!(buckets, 1, "slot {index} must sit in exactly one bucket");
        }
    }

    #[test]
    fn aborted_frames_show_up_as_unmarked_time() {
        let mut profile = FrameProfile::default();
        // A frame whose tail never stamped: the span is longer than the buckets.
        profile.record(
            spread(
                Duration::from_micros(100),
                Duration::from_micros(100),
                Duration::from_micros(100),
                Duration::ZERO,
            ),
            Duration::from_micros(500),
        );
        let report = profile.expect_report();
        assert_eq!(report.tail_ms, 0.0);
        assert_eq!(report.unmarked_ms, 0.2);
    }

    impl FrameProfile {
        /// Test helper: unwrap the report so the tests read as arithmetic.
        fn expect_report(&self) -> FrameProfileReport {
            self.report().expect("a profile with frames reports")
        }
    }
}
