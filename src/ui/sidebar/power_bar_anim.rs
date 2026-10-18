//! PowerClass's retained segment animation and native draw commands.
//!
//! Original `63F850`, `63F960`, `63FEA0` and `63FB20` execute in
//! `tools/sidebar_oracle/power_draw.py --state`. Only presentation state lives
//! here; House power accounting and building membership retain their owners.

use crate::sim::snapshot::saved_local_view::{SavedLocalViewError, SavedPowerBar};
use crate::sim::timer::CdTimer;
use crate::util::native_x87::{MaskedX87Chop53 as Fpu, MaskedX87Ordering, NativeF64Bits};

const SEGMENT_HEIGHT_PX: i32 = 3;
const FLASH_COUNT: i32 = 10;
const FLASH_BUCKETS: i32 = 3;

/// Presentation state owned by PowerClass (`+1510..+1540`). Counts are ordered
/// surplus/green, output/yellow, drain/red. Timers use 16-ms wall-clock buckets,
/// through the shared signed countdown primitive, rather than simulation ticks.
#[derive(Debug, Clone)]
pub struct PowerBarAnimState {
    counts: [i32; 3],
    max_segments: i32,
    cached_output: i32,
    cached_drain: i32,
    flashes_remaining: i32,
    flash_timer: CdTimer,
    slide_timer: CdTimer,
    animating: bool,
}

impl Default for PowerBarAnimState {
    fn default() -> Self {
        Self::new()
    }
}

impl PowerBarAnimState {
    pub fn new() -> Self {
        Self {
            counts: [0; 3],
            max_segments: 0,
            cached_output: -1,
            cached_drain: -1,
            flashes_remaining: 0,
            flash_timer: CdTimer::started(0, 0),
            slide_timer: CdTimer::started(0, 0),
            animating: false,
        }
    }

    /// MouseSave5BE715 copies the retained state without pausing Flash. Its
    /// opaque timer words are not semantic fields. The existing Rust draw path
    /// rebuilds meter commands from this owner, without a native dirty latch.
    pub(crate) fn saved(&self) -> SavedPowerBar {
        SavedPowerBar {
            counts_surplus_output_drain: self.counts,
            cached_output: self.cached_output,
            cached_drain: self.cached_drain,
            flashes_remaining: self.flashes_remaining,
            flash_timer: [self.flash_timer.start_frame(), self.flash_timer.duration()],
            animating: self.animating,
        }
    }

    /// Prepare the incoming Power owner before mutating a live app. Ordinary
    /// own writers keep nonnegative bands and Flash0..10. A resize may leave
    /// more bands than today's capacity; caches and timer words stay verbatim.
    pub(crate) fn from_saved(saved: SavedPowerBar) -> Result<Self, SavedLocalViewError> {
        if saved
            .counts_surplus_output_drain
            .iter()
            .any(|&count| count < 0)
            || saved
                .counts_surplus_output_drain
                .iter()
                .try_fold(0_i32, |total, &count| total.checked_add(count))
                .is_none()
        {
            return Err(SavedLocalViewError::InvalidPowerState {
                field: "band counts",
            });
        }
        if !(0..=FLASH_COUNT).contains(&saved.flashes_remaining) {
            return Err(SavedLocalViewError::InvalidPowerState {
                field: "flash count",
            });
        }
        Ok(Self {
            counts: saved.counts_surplus_output_drain,
            cached_output: saved.cached_output,
            cached_drain: saved.cached_drain,
            flashes_remaining: saved.flashes_remaining,
            flash_timer: CdTimer::from_raw(saved.flash_timer[0], saved.flash_timer[1]),
            // NoInit replaces this pair at the successful app commit.
            slide_timer: CdTimer::default(),
            max_segments: 0,
            animating: saved.animating,
        })
    }

    /// SidebarNoInit6A4F89..94 rearms only slide, preserving the loaded Flash,
    /// bands, caches and active flag. Use the same effective HUD clock as AI.
    pub(crate) fn reanchor_after_load(&mut self, wall_ms: u64) {
        self.slide_timer.start(((wall_ms as u32) >> 4) as i32, 0);
    }

    /// Fresh-scenario clear `63F730`. Geometry belongs to Sidebar, and is not
    /// reset here. Native save/load retains Power fields and has a different
    /// NoInit path; it must not use this clear.
    pub fn reset(&mut self, wall_ms: u64) {
        let capacity = self.max_segments;
        *self = Self::new();
        self.max_segments = capacity;
        let bucket = ((wall_ms as u32) >> 4) as i32;
        self.flash_timer.start(bucket, 0);
        self.slide_timer.start(bucket, 0);
    }

    /// Set the available pixel budget (native strip height plus three).
    /// A native geometry write does not clamp the retained counts or start an
    /// animation; `fit_strip` owns VERA's mid-match resizes.
    pub fn set_max_segments(&mut self, bar_height_px: i32) {
        self.max_segments = bar_height_px / SEGMENT_HEIGHT_PX;
    }

    /// The app's per-frame strip write. gamemd's only strip-height writer
    /// (`6A5130`, from Sidebar init and gadget layout) derives it from the
    /// screen or tactical view, which only VERA changes mid-match (window,
    /// fullscreen, sidebar scale). When an established strip changes, the
    /// retained totals take their settled bands for the new budget (`63F960`),
    /// so nothing draws past the strip. A layout change is not a Power change:
    /// no flash, slide or AI visit. A fresh or loaded owner's first budget only
    /// sets capacity, and a fresh clear has no totals (-1) to refit.
    pub fn fit_strip(&mut self, bar_height_px: i32, theoretical_total: impl FnOnce() -> i32) {
        let established = self.max_segments;
        self.set_max_segments(bar_height_px);
        if established == 0 || established == self.max_segments || self.cached_output < 0 {
            return;
        }
        self.counts =
            self.compute_targets(self.cached_output, self.cached_drain, theoretical_total());
    }

    /// One ordinary visible Sidebar AI visit (`63FEA0..640385`). The caller
    /// supplies its wall clock even when no simulation frame was admitted.
    /// The rated-list read runs only after the native unchanged/idle return.
    pub fn update(
        &mut self,
        output: i32,
        drain: i32,
        theoretical_total: impl FnOnce() -> i32,
        wall_ms: u64,
    ) {
        let bucket = ((wall_ms as u32) >> 4) as i32;
        // Flashing waits for the entire segment transition, then advances at
        // most once per visit. A long draw stall does not catch up ten blinks.
        if !self.animating && self.flashes_remaining > 0 && self.flash_timer.expired(bucket) {
            self.flashes_remaining -= 1;
            self.flash_timer.start(bucket, FLASH_BUCKETS);
        }
        let changed = output != self.cached_output || drain != self.cached_drain;
        if !changed && !self.animating {
            return;
        }
        if changed {
            self.animating = true;
            self.flashes_remaining = FLASH_COUNT;
            self.flash_timer.start(bucket, FLASH_BUCKETS);
        }
        self.cached_output = output;
        self.cached_drain = drain;
        let theoretical_total = theoretical_total();
        let targets = self.compute_targets(output, drain, theoretical_total);
        if !self.slide_timer.expired(bucket) {
            return;
        }

        // 640064 chooses drain, surplus, output. Each move then performs the
        // opposite transfer (63FE30: drain/surplus/output;
        // 63FDC0: surplus/drain/output). This preserves fill during a color
        // transfer, while allowing a changing theoretical total to grow/shrink.
        self.animating = false;
        if let Some(index) = [2, 0, 1]
            .into_iter()
            .find(|&i| self.counts[i] != targets[i])
        {
            self.animating = true;
            if self.counts[index] > targets[index] {
                self.counts[index] -= 1;
                if let Some(other) = [2, 0, 1].into_iter().find(|&i| self.counts[i] < targets[i]) {
                    self.counts[other] += 1;
                }
            } else {
                self.counts[index] += 1;
                if let Some(other) = [0, 2, 1].into_iter().find(|&i| self.counts[i] > targets[i]) {
                    self.counts[other] -= 1;
                }
            }
        }
        if self.animating {
            let filled = self.filled_segments(theoretical_total);
            // 63F8F0 and its inline AI consumer64032B: signed sum, no clamp
            // to zero; truncate(min(displayed, filled) / filled * 5).
            let displayed = self.counts.iter().copied().fold(0_i32, i32::wrapping_add);
            let ratio = Fpu::div(Fpu::load_i32(displayed.min(filled)), Fpu::load_i32(filled));
            let duration = Fpu::ftol_i32_low_masked(Fpu::mul(ratio, Fpu::load_i32(5)));
            self.slide_timer.start(bucket, duration);
        }
    }

    pub fn is_flashing(&self) -> bool {
        self.flashes_remaining > 0 && self.flashes_remaining % 2 == 0
    }

    pub fn max_segments(&self) -> i32 {
        self.max_segments
    }

    /// Original PowerClass draw loop `63FB20..63FDA5`, after the strips.
    /// A blink consumes the first segment of the first nonempty band.
    pub fn visit_draw_segments(&self, mut emit: impl FnMut(u8, i32)) {
        let mut row = 0;
        let filled = self.counts.iter().copied().fold(0_i32, i32::wrapping_add);
        for _ in 0..self.max_segments.wrapping_sub(filled) {
            emit(0, row);
            row += 1;
        }
        let mut consumed = 0;
        if self.is_flashing() {
            emit(4, row);
            row += 1;
            consumed = 1;
        }
        for (frame, count) in [1, 2, 3].into_iter().zip(self.counts) {
            if count > 0 {
                for _ in consumed..count {
                    emit(frame, row);
                    row += 1;
                }
                consumed = 0;
            }
        }
    }

    /// Calc_Segments63F850. The native clamp guarantees at least one filled
    /// segment. It applies after conversion, including masked-invalid FTOL.
    fn filled_segments(&self, theoretical_total: i32) -> i32 {
        let scale = Fpu::load_i32(400); // original double7ED8C8
        let ratio = Fpu::div(scale, Fpu::add(Fpu::load_i32(theoretical_total), scale));
        let empty = Fpu::ftol_i32_low_masked(Fpu::mul(ratio, Fpu::load_i32(self.max_segments)));
        let empty = empty.max(0).min(self.max_segments.wrapping_sub(1));
        self.max_segments.wrapping_sub(empty)
    }

    /// Calc_Power_Distribution63F960. Reuse the existing deterministic PC53/chop
    /// owner: a native 1/3 * 3 can remain below one before FTOL, changing visible
    /// color bands. The native --state controls pin this arithmetic trap.
    fn compute_targets(&self, output: i32, drain: i32, theoretical_total: i32) -> [i32; 3] {
        let filled = Fpu::load_i32(self.filled_segments(theoretical_total));
        let spare = output.wrapping_sub(drain); //63F9AB subtracts signed dwords
        let yellow = spare.clamp(0, 100); // double100 at7E2AC0
        let green = if spare < 0 { 0 } else { spare - yellow };
        let drain_value = Fpu::load_i32(drain);
        let yellow_value = Fpu::load_i32(yellow);
        let green_value = Fpu::load_i32(green);
        let total = Fpu::add(Fpu::add(drain_value, yellow_value), green_value);
        let (drain_fraction, green_fraction, yellow_fraction) =
            if Fpu::compare(total, Fpu::load_i32(0)) == MaskedX87Ordering::Greater {
                (
                    Fpu::div(drain_value, total),
                    Fpu::div(green_value, total),
                    Fpu::div(yellow_value, total),
                )
            } else {
                (Fpu::load_i32(1), Fpu::load_i32(0), Fpu::load_i32(0))
            };
        let raw_drain = Fpu::mul(filled, drain_fraction);
        let raw_yellow = Fpu::mul(filled, yellow_fraction);
        let raw_green = Fpu::mul(filled, green_fraction);
        let drain_count = Fpu::ftol_i32_low_masked(raw_drain);
        let yellow_count = Fpu::ftol_i32_low_masked(raw_yellow);
        let green_count = Fpu::ftol_i32_low_masked(raw_green);
        // 63FAE8..63FB0D adds each fractional remainder back to drain in this
        // order, then the stored double0.01 at7E3808 before its final FTOL.
        let remainder = Fpu::add(
            Fpu::sub(raw_green, Fpu::load_i32(green_count)),
            Fpu::sub(raw_yellow, Fpu::load_i32(yellow_count)),
        );
        let remainder = Fpu::add(Fpu::sub(raw_drain, Fpu::load_i32(drain_count)), remainder);
        let epsilon = Fpu::load_f64(NativeF64Bits::from_bits(0x3f84_7ae1_47ae_147b));
        let drain_count = Fpu::ftol_i32_low_masked(Fpu::add(
            Fpu::add(remainder, Fpu::load_i32(drain_count)),
            epsilon,
        ));
        [green_count, yellow_count, drain_count]
    }
}

#[cfg(test)]
mod native_draw_tests {
    use super::*;
    use serde::Deserialize;
    #[derive(Deserialize)]
    struct Packet {
        cases: Vec<Case>,
    }
    #[derive(Deserialize)]
    struct Case {
        side: usize,
        strip_height: i32,
        counts_surplus_output_drain: [i32; 3],
        flashes: i32,
        frame_x_y: Vec<[i32; 3]>,
    }
    #[test]
    fn original_1215_power_draw_sequences_match() {
        let packet: Packet = serde_json::from_str(crate::test_fixture::text(
            "tools/sidebar_oracle/power_draw.json",
        ))
        .unwrap();
        assert_eq!(packet.cases.len(), 1215);
        for c in packet.cases {
            let a = PowerBarAnimState {
                counts: c.counts_surplus_output_drain,
                max_segments: (c.strip_height + 3) / 3,
                flashes_remaining: c.flashes,
                ..Default::default()
            };
            let mut commands = Vec::new();
            a.visit_draw_segments(|frame, row| {
                commands.push([
                    i32::from(frame),
                    if c.side == 0 { 5 } else { 0 },
                    227 + 3 * row,
                ])
            });
            assert_eq!(
                commands, c.frame_x_y,
                "side{} height{} counts{:?} flashes{}",
                c.side, c.strip_height, c.counts_surplus_output_drain, c.flashes
            );
        }
    }
}

#[cfg(test)]
mod native_state_tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct Packet {
        rated_lists: Vec<RatedList>,
        numeric: Vec<Numeric>,
        histories: Vec<History>,
        loaded_histories: Vec<LoadedHistory>,
    }

    #[derive(Deserialize)]
    struct RatedList {
        name: String,
        total: i32,
        strip_height: i32,
        filled: i32,
    }

    #[derive(Deserialize)]
    struct Numeric {
        input: [i32; 4],
        counts: [i32; 3],
    }

    #[derive(Deserialize)]
    struct History {
        name: String,
        visits: Vec<Visit>,
    }

    #[derive(Deserialize)]
    struct Retained {
        counts: [i32; 3],
        flash: i32,
        flash_timer: [i32; 2],
        slide_timer: [i32; 2],
        cached: [i32; 2],
        animating: bool,
    }

    impl Retained {
        fn saved(&self) -> SavedPowerBar {
            SavedPowerBar {
                counts_surplus_output_drain: self.counts,
                cached_output: self.cached[1],
                cached_drain: self.cached[0],
                flashes_remaining: self.flash,
                flash_timer: self.flash_timer,
                animating: self.animating,
            }
        }
    }

    #[derive(Deserialize)]
    struct Visit {
        input: [i64; 7],
        #[serde(flatten)]
        retained: Retained,
        fill_calls: usize,
        frames: Vec<u8>,
    }

    #[derive(Deserialize)]
    struct LoadedHistory {
        name: String,
        input: [i64; 7],
        prior: Retained,
        no_init: Retained,
        visits: Vec<Visit>,
    }

    fn assert_retained(anim: &PowerBarAnimState, expected: &Retained, label: &str) {
        assert_eq!(anim.saved(), expected.saved(), "{label}");
        assert_eq!(
            [anim.slide_timer.start_frame(), anim.slide_timer.duration()],
            expected.slide_timer,
            "{label} slide"
        );
    }

    fn exercise_visit(anim: &mut PowerBarAnimState, c: &Visit, label: &str) {
        let [wall_ms, height, output, drain, total, visible, reset] = c.input;
        if reset != 0 {
            anim.reset(wall_ms as u64);
        }
        anim.set_max_segments(height as i32 + 3);
        let mut supplied = 0;
        if visible != 0 {
            anim.update(
                output as i32,
                drain as i32,
                || {
                    supplied += 1;
                    total as i32
                },
                wall_ms as u64,
            );
        }
        // Native can repeat the pure query; Rust reads the stable list once.
        assert_eq!(supplied > 0, c.fill_calls > 0, "{label} list admission");
        assert_retained(anim, &c.retained, label);
        let mut frames = Vec::new();
        if visible != 0 {
            anim.visit_draw_segments(|frame, _row| frames.push(frame));
        }
        assert_eq!(frames, c.frames, "{label} draw");
    }

    fn packet() -> Packet {
        serde_json::from_str(crate::test_fixture::text(
            "tools/sidebar_oracle/power_state.json",
        ))
        .unwrap()
    }

    #[test]
    fn original_power_numeric_controls_match() {
        let packet = packet();
        for row in packet.rated_lists {
            let mut anim = PowerBarAnimState::new();
            anim.set_max_segments(row.strip_height + 3);
            assert_eq!(anim.filled_segments(row.total), row.filled, "{}", row.name);
        }
        for c in packet.numeric {
            let [height, output, drain, total] = c.input;
            let mut anim = PowerBarAnimState::new();
            anim.set_max_segments(height + 3);
            assert_eq!(
                anim.compute_targets(output, drain, total),
                c.counts,
                "native distribution {input:?}",
                input = c.input,
            );
        }
    }

    #[test]
    fn retail_russian_opening_targets_consume_native_controls() {
        let Some(retail) =
            crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
        else {
            return;
        };
        let power = retail.rules.object("NAPOWR").unwrap().power;
        let barracks_drain = retail.rules.object("NAHAND").unwrap().power.wrapping_neg();
        let refinery_drain = retail.rules.object("NAREFN").unwrap().power.wrapping_neg();
        let packet = packet();
        let mut anim = PowerBarAnimState::new();
        anim.set_max_segments(303);
        for drain in [
            0,
            barracks_drain,
            barracks_drain.wrapping_add(refinery_drain),
        ] {
            let total = power.wrapping_abs().wrapping_add(drain);
            let case = packet
                .numeric
                .iter()
                .find(|case| case.input == [300, power, drain, total])
                .expect("layered retail Power values must have native input evidence");
            assert_eq!(anim.compute_targets(power, drain, total), case.counts);
        }
    }

    #[test]
    fn original_power_constructor_ai_draw_histories_match() {
        let mut queried = 0;
        let mut flash_only = 0;
        for history in packet().histories {
            let mut anim = PowerBarAnimState::new();
            for (index, c) in history.visits.iter().enumerate() {
                exercise_visit(&mut anim, c, &format!("{} visit{index}", history.name));
                if c.input[5] != 0 {
                    queried += usize::from(c.fill_calls > 0);
                    flash_only += usize::from(c.fill_calls == 0 && c.retained.flash > 0);
                }
            }
        }
        assert!(
            queried > 0 && flash_only > 0,
            "native query/flash-only coverage"
        );
    }

    #[test]
    fn original_saved_power_no_init_and_continuation_match() {
        let histories = packet().loaded_histories;
        assert_eq!(histories.len(), 8);
        let mut visits = 0;
        for history in histories {
            // Transport the semantic projection, without old geometry/slide.
            let bytes = crate::util::save_codec::serialize(&history.prior.saved()).unwrap();
            let saved = crate::util::save_codec::deserialize(&bytes).unwrap();
            let mut anim = PowerBarAnimState::from_saved(saved).unwrap();
            anim.set_max_segments(history.input[1] as i32 + 3);
            anim.reanchor_after_load(history.input[0] as u64);
            assert_retained(&anim, &history.no_init, &format!("{} NoInit", history.name));
            for (index, c) in history.visits.iter().enumerate() {
                exercise_visit(
                    &mut anim,
                    c,
                    &format!("{} loaded visit{index}", history.name),
                );
                visits += 1;
            }
        }
        assert_eq!(visits, 22);
    }

    #[test]
    fn malformed_saved_counters_are_rejected_without_normalization() {
        let good = packet()
            .loaded_histories
            .into_iter()
            .find(|row| row.name == "retained-active")
            .unwrap()
            .prior
            .saved();
        for (counts, flash) in [
            ([-1, 0, 0], 4),
            ([i32::MAX, 1, 0], 4),
            ([0, 0, 3], -1),
            ([0, 0, 3], 11),
        ] {
            let mut saved = good.clone();
            saved.counts_surplus_output_drain = counts;
            saved.flashes_remaining = flash;
            assert!(matches!(
                PowerBarAnimState::from_saved(saved),
                Err(SavedLocalViewError::InvalidPowerState { .. })
            ));
        }
    }
}

#[cfg(test)]
mod strip_resize_tests {
    use super::*;

    const OUTPUT: i32 = 200;
    const DRAIN: i32 = 100;
    const TOTAL: i32 = 300;

    fn settle(anim: &mut PowerBarAnimState, wall_ms: &mut u64) {
        for _ in 0..10_000 {
            anim.update(OUTPUT, DRAIN, || TOTAL, *wall_ms);
            *wall_ms += 16;
            if !anim.animating && anim.flashes_remaining == 0 {
                return;
            }
        }
        panic!("power bar did not settle");
    }

    fn settled_bands(bar_height_px: i32) -> [i32; 3] {
        let mut fresh = PowerBarAnimState::new();
        fresh.set_max_segments(bar_height_px);
        fresh.compute_targets(OUTPUT, DRAIN, TOTAL)
    }

    #[test]
    fn window_resize_refits_settled_bands_inside_the_strip() {
        let mut anim = PowerBarAnimState::new();
        let mut wall_ms = 0;
        // The first budget only sets capacity: Power still starts from zero.
        anim.fit_strip(303, || TOTAL);
        assert_eq!(anim.counts, [0; 3]);
        settle(&mut anim, &mut wall_ms);
        assert_eq!(anim.counts, settled_bands(303));

        for (bar_height_px, label) in [(93, "shrink"), (303, "grow")] {
            anim.fit_strip(bar_height_px, || TOTAL);
            assert_eq!(anim.counts, settled_bands(bar_height_px), "{label}");
            assert!(
                !anim.animating && anim.flashes_remaining == 0,
                "{label}: a layout change neither slides nor flashes"
            );
            let capacity = anim.max_segments();
            let mut rows = 0;
            anim.visit_draw_segments(|_, row| {
                assert!(row < capacity, "{label}: row {row} past the strip");
                rows += 1;
            });
            assert_eq!(rows, capacity, "{label}");
            // The next ordinary visit sees settled Power and stays idle.
            anim.update(
                OUTPUT,
                DRAIN,
                || panic!("idle visit read the rated list"),
                wall_ms,
            );
            assert_eq!(anim.counts, settled_bands(bar_height_px), "{label} idle");
        }
    }

    #[test]
    fn fresh_and_loaded_power_keep_their_bands_on_the_first_budget() {
        // Fresh-scenario clear: no retained totals to refit, even after a
        // between-scenario resize.
        let mut anim = PowerBarAnimState::new();
        anim.set_max_segments(303);
        anim.reset(0);
        anim.fit_strip(93, || TOTAL);
        assert_eq!(anim.counts, [0; 3]);

        // A load retains its bands past a smaller strip, as native NoInit
        // does (`settled-resize-load`); restore sets the budget, and the next
        // frame's write is unchanged.
        let mut anim = PowerBarAnimState::from_saved(SavedPowerBar {
            counts_surplus_output_drain: [5, 5, 6],
            cached_output: OUTPUT,
            cached_drain: DRAIN,
            flashes_remaining: 0,
            flash_timer: [0, 0],
            animating: false,
        })
        .unwrap();
        anim.set_max_segments(15);
        anim.fit_strip(15, || TOTAL);
        assert_eq!(anim.counts, [5, 5, 6]);
    }
}
