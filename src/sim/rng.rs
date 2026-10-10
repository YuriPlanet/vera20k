//! Deterministic PRNG used by simulation systems.
//!
//! Mirrors gamemd.exe `Random::RandomRanged`/scenario RNG shape for
//! gameplay-visible randomness: one auditable stream, 250-word XOR-lag state,
//! inclusive sorted ranged draws, and rejection sampling.

use std::hash::{Hash, Hasher};

use crate::rng_continuation::MapGenRngContinuation;
use crate::util::native_random;

/// The `double` at `0x007E3570` that scales a `RandomRanged(0, 0x7ffffffe)`
/// draw onto `[0, 1]` for a unit-interval probability gate (Spark spawns, the
/// `CrewEscape=` roll). Raw bytes `00 00 40 00 00 00 00 3E`, i.e.
/// `1.0 / 2147483646.0` — the reciprocal of the draw's own inclusive top, NOT
/// `2^-31`. The two differ by `2^-30` relative, which is a real bias in a
/// deterministic gate, so this is carried as bits and never recomputed.
pub(crate) const RANDOM_RANGED_UNIT_SCALE: crate::util::native_x87::NativeF64Bits =
    crate::util::native_x87::NativeF64Bits::from_bits(0x3e00_0000_0040_0000);

const RNG_TABLE_LEN: usize = native_random::STATE_WORDS;

#[cfg(test)]
thread_local! {
    static DRAW_TRACE: std::cell::RefCell<Option<Vec<serde_json::Value>>> = const { std::cell::RefCell::new(None) };
    static DRAW_LOGIC_OBJECT: std::cell::Cell<Option<u64>> = const { std::cell::Cell::new(None) };
}

/// Observation only: identify the live Logic caller even when several objects
/// enter the same mission handler. The guard restores an enclosing callback's
/// identity on normal return, early exit or panic; it never enters sim state.
#[cfg(test)]
pub(crate) struct DrawLogicObject(Option<u64>);

#[cfg(test)]
impl Drop for DrawLogicObject {
    fn drop(&mut self) {
        DRAW_LOGIC_OBJECT.set(self.0);
    }
}

#[cfg(test)]
pub(crate) fn observe_logic_object(id: u64) -> DrawLogicObject {
    DrawLogicObject(DRAW_LOGIC_OBJECT.replace(Some(id)))
}

/// Test-only observation of real production calls. It neither supplies words
/// nor changes stream cursors; callers keep traces outside serialized state.
#[cfg(test)]
pub(crate) fn trace_draws<T>(run: impl FnOnce() -> T) -> (T, Vec<serde_json::Value>) {
    DRAW_TRACE.with_borrow_mut(|trace| {
        assert!(trace.is_none(), "nested RNG traces");
        *trace = Some(Vec::new());
    });
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            DRAW_TRACE.with_borrow_mut(|trace| *trace = None);
        }
    }
    let reset = Reset;
    let result = run();
    let trace = DRAW_TRACE.with_borrow_mut(|trace| trace.take().unwrap());
    drop(reset);
    (result, trace)
}

/// Deterministic simulation RNG.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SimRng {
    disabled: u8,
    index_a: i32,
    index_b: i32,
    state: Vec<u32>,
}

/// Borrowed process Main draws. This capability cannot clone, seed, replace
/// or retain the cursor; its owner keeps the complete RNG state.
pub(crate) struct MainRngDraws<'a> {
    rng: &'a mut SimRng,
}

impl<'a> MainRngDraws<'a> {
    pub(crate) fn borrow(rng: &'a mut SimRng) -> Self {
        Self { rng }
    }
    pub(crate) fn next_u32(&mut self) -> u32 {
        self.rng.next_u32()
    }
    pub(crate) fn ranged(&mut self, low: i32, high: i32) -> i32 {
        self.rng.next_range_i32_inclusive(low, high)
    }
}

/// Allocation-free logical view of the native-significant RNG fields.
///
/// Native padding is deliberately absent: Rust has no verified storage or
/// comparison policy for those three bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SimRngLogicalView<'a> {
    pub disabled: u8,
    pub index_a: i32,
    pub index_b: i32,
    pub words: &'a [u32],
}

/// Owned logical RNG evidence captured at a named simulation boundary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SimRngLogicalState {
    pub disabled: u8,
    pub index_a: i32,
    pub index_b: i32,
    pub words: [u32; RNG_TABLE_LEN],
}

impl SimRng {
    /// Create a new RNG with the given seed.
    pub fn new(seed: u64) -> Self {
        Self {
            disabled: 0,
            index_a: 0,
            index_b: native_random::LAG as i32,
            state: Vec::from(native_random::seeded_words(seed as u32)),
        }
    }

    /// Adopt the exact post-RMG `g_MapGenRng` cursor without replaying draws.
    ///
    /// The two implementations intentionally keep separate draw code, but the
    /// native 250-word state and lag cursors are the same logical object at the
    /// launch `.SED` generation -> live-scenario boundary.
    pub(crate) fn from_mapgen_continuation(continuation: MapGenRngContinuation) -> Self {
        let (words, index_a, index_b) = continuation.into_native_parts();
        Self {
            disabled: 0,
            index_a: i32::try_from(index_a).expect("MapGen cursor A fits native i32"),
            index_b: i32::try_from(index_b).expect("MapGen cursor B fits native i32"),
            state: Vec::from(words),
        }
    }

    /// Compact deterministic fingerprint of the full internal state.
    ///
    /// This is for tests and debug comparisons. Use `hash_state` when feeding
    /// the authoritative world hash so every field contributes directly.
    pub fn state(&self) -> u64 {
        let mut hash = crate::util::fnv::FNV1A64_OFFSET_BASIS;
        let mut mix = |value: u64| {
            hash = crate::util::fnv::fnv1a64_fold_bytes(hash, &value.to_le_bytes());
        };
        mix(u64::from(self.disabled));
        mix(self.index_a as u32 as u64);
        mix(self.index_b as u32 as u64);
        for &word in &self.state {
            mix(u64::from(word));
        }
        hash
    }

    /// Borrow every logical field without copying or exposing mutation.
    pub fn logical_view(&self) -> SimRngLogicalView<'_> {
        SimRngLogicalView {
            disabled: self.disabled,
            index_a: self.index_a,
            index_b: self.index_b,
            words: &self.state,
        }
    }

    /// The native Random2Class object bytes (`+0x00` disabled dword, the two
    /// indices, then the table), as hex: the form the Unicorn oracles record
    /// Scenario RNG states in.
    #[cfg(test)]
    pub(crate) fn native_state_hex(&self) -> String {
        let mut bytes = Vec::with_capacity(0x3f4);
        bytes.extend_from_slice(&u32::from(self.disabled).to_le_bytes());
        bytes.extend_from_slice(&self.index_a.to_le_bytes());
        bytes.extend_from_slice(&self.index_b.to_le_bytes());
        for word in &self.state {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    /// Import a pinned original Random2Class comparison boundary. This reads
    /// logical fields only; it does not seed or execute a second RNG port.
    #[cfg(test)]
    pub(crate) fn from_native_state_hex_for_test(hex: &str) -> Self {
        assert_eq!(hex.len(), 0x3f4 * 2, "native Random object byte count");
        let bytes: Vec<u8> = hex
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect();
        assert_eq!(&bytes[1..4], &[0, 0, 0], "unmodeled native padding");
        let word =
            |offset: usize| u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
        Self {
            disabled: bytes[0],
            index_a: word(4) as i32,
            index_b: word(8) as i32,
            state: (12..0x3f4).step_by(4).map(word).collect(),
        }
    }

    /// Copy every logical field into immutable boundary evidence.
    pub fn logical_state(&self) -> SimRngLogicalState {
        let mut words = [0; RNG_TABLE_LEN];
        words.copy_from_slice(&self.state);
        SimRngLogicalState {
            disabled: self.disabled,
            index_a: self.index_a,
            index_b: self.index_b,
            words,
        }
    }

    /// The first seeded stream whose next `RandomRanged(low, high)` answers
    /// `value`: how oracle replays make the Rust draw the native answer.
    #[cfg(test)]
    pub(crate) fn answering(low: i32, high: i32, value: i32) -> Self {
        (0..)
            .map(Self::new)
            .find(|rng| rng.clone().next_range_i32_inclusive(low, high) == value)
            .expect("some seed answers every in-range value")
    }

    /// Test/debug accessor for the secondary lag index. Used by the two-stream
    /// routing tests to assert both streams seed to the gamemd `index_b = 0x67`
    /// start. Not part of the gameplay API.
    #[cfg(test)]
    pub(crate) fn index_b(&self) -> i32 {
        self.index_b
    }

    /// Hash the complete RNG state for deterministic replay/desync checks.
    pub fn hash_state(&self, hasher: &mut impl Hasher) {
        self.disabled.hash(hasher);
        self.index_a.hash(hasher);
        self.index_b.hash(hasher);
        self.state.hash(hasher);
    }

    /// Advance and return next random u64.
    pub fn next_u64(&mut self) -> u64 {
        let lo = u64::from(self.next_u32());
        let hi = u64::from(self.next_u32());
        lo | (hi << 32)
    }

    /// Next random u32.
    pub fn next_u32(&mut self) -> u32 {
        if self.disabled != 0 {
            return 0;
        }

        let a = self.index_a as usize;
        let b = self.index_b as usize;
        let (mut lead, mut lagged) = (a, b);
        let value = native_random::draw(&mut self.state, &mut lead, &mut lagged);
        self.index_a = lead as i32;
        self.index_b = lagged as i32;

        #[cfg(test)]
        DRAW_TRACE.with_borrow_mut(|trace| {
            if let Some(trace) = trace {
                trace.push(serde_json::json!({
                    "before_indices": [a, b], "value": value,
                    "logic_object": DRAW_LOGIC_OBJECT.get(),
                    "callers": std::backtrace::Backtrace::force_capture().to_string(),
                }));
            }
        });

        value
    }

    /// Random integer in [0, max_exclusive). Returns 0 for max_exclusive=0.
    pub fn next_range_u32(&mut self, max_exclusive: u32) -> u32 {
        if max_exclusive == 0 {
            return 0;
        }
        self.next_range_u32_inclusive(0, max_exclusive - 1)
    }

    /// Take the high two bits of one raw draw, returning `0..=3`.
    ///
    /// This is the bridge walkers' MapGen `598030(0,3)` operation under the
    /// original startup PC53/chop mode. Unlike the Scenario range helper, it
    /// neither masks the low bits nor rejection-samples. A disabled stream
    /// still receives its one Next call, which returns zero without advancing.
    /// Native boundary values and complete states: `tools/spatial_oracle/mapgen_range`.
    ///
    /// This deliberately exposes only the operation our repair caller needs.
    /// Wider/reversed native intervals and disturbed ambient FPU modes can
    /// round or retry differently; the former generic integer approximation
    /// was not valid for those inputs. RMG has its own range implementation.
    pub(crate) fn next_high_two_bits(&mut self) -> u8 {
        (self.next_u32() >> 30) as u8
    }

    /// Signed form of `Random__RandomRanged @ 0x0065C7E0`. The native compares
    /// its two `int` bounds signed, swaps reversed bounds, and rejection-samples
    /// the masked span exactly like [`Self::next_range_u32_inclusive`]; a rules
    /// value can make `high` negative (`OreTwinkleChance - 1` with chance `<= 0`),
    /// which the unsigned form would misread as a huge span. Spans of
    /// `0x80000000` or more never terminate natively (zero mask); the same
    /// `lo + 0x80000000` guard as the unsigned form stands in for that
    /// unreachable custom-data case.
    pub fn next_range_i32_inclusive(&mut self, low: i32, high: i32) -> i32 {
        let (lo, hi) = if low <= high {
            (low, high)
        } else {
            (high, low)
        };
        if lo == hi {
            return lo;
        }
        let span = hi.wrapping_sub(lo) as u32;
        if span >= 0x7FFF_FFFF {
            return lo.wrapping_add(i32::MIN);
        }
        let mask = u32::MAX >> span.leading_zeros();
        loop {
            let sample = self.next_u32() & mask;
            if sample <= span {
                return lo.wrapping_add(sample as i32);
            }
        }
    }

    /// Random integer in `[low, high]` inclusive on both ends.
    /// Sorts reversed bounds and consumes no draw when the bounds are equal.
    /// Mirrors binary `Random__RandomRanged(low, high)` for ordinary spans.
    pub fn next_range_u32_inclusive(&mut self, low: u32, high: u32) -> u32 {
        let (lo, hi) = if low <= high {
            (low, high)
        } else {
            (high, low)
        };
        if lo == hi {
            return lo;
        }

        let span = hi.wrapping_sub(lo);
        if span >= 0x7FFF_FFFF {
            return lo.wrapping_add(0x8000_0000);
        }

        // Mask one bit wider than the span's highest set bit, matching the
        // rejection-sampling mask 2^(msb+1)-1. next_power_of_two() is wrong
        // because it returns the span itself when span is already a power of
        // two, producing a mask one bit too short (e.g. span=4 -> 3 instead of
        // 7): that biases the output (the inclusive top is never reached) and
        // changes how many raw draws are consumed. span is guaranteed in
        // 1..=0x7FFF_FFFE here (lo==hi early return handles span==0; the
        // span >= 0x7FFF_FFFF guard above handles the top), so leading_zeros is
        // 1..=31 and the shift never reaches 32.
        let mask = u32::MAX >> span.leading_zeros();
        loop {
            let sample = self.next_u32() & mask;
            if sample <= span {
                return lo.wrapping_add(sample);
            }
        }
    }

    /// Raw signed-abs remainder draw: `abs((next_u32() as i32) % n)`, one draw.
    ///
    /// Models gamemd's particle-lifetime / fire-insert-offset primitive: a
    /// single raw draw taken as a signed int, `% n`, then absolute value.
    /// Unlike `next_range_u32` (mask-and-reject), this consumes EXACTLY ONE draw
    /// — no rejection loop — so the shared scenario cursor advances by one per
    /// call. That fixed advance is the parity point: rejection sampling shifts
    /// the cursor a variable amount and desyncs every later consumer that tick.
    /// Returns 0 for `n == 0` (the original would divide-by-zero here; callers
    /// guard with `.max(1)`, so this branch never fires on stock systems).
    pub fn next_raw_abs_modulo(&mut self, n: u32) -> u32 {
        if n == 0 {
            return 0;
        }
        ((self.next_u32() as i32) % n as i32).unsigned_abs()
    }

    /// Raw signed remainder draw: `(next_u32() as i32) % n`, one draw, may be
    /// negative.
    ///
    /// Models gamemd's particle jitter / spawn-offset primitive (`CDQ; IDIV`,
    /// no absolute value): a negative remainder yields a negative offset, so the
    /// output spans `-(n-1)..=n-1`. Same single-draw cursor advance as
    /// `next_raw_abs_modulo`; the only difference is the sign is preserved.
    /// Returns 0 for `n == 0`.
    pub fn next_raw_modulo_signed(&mut self, n: u32) -> i32 {
        if n == 0 {
            return 0;
        }
        (self.next_u32() as i32) % n as i32
    }
}

#[cfg(test)]
mod tests {
    use super::SimRng;

    #[test]
    fn high_two_bits_match_original_mapgen_range_and_all_retained_words() {
        fn retained(hex: &str) -> SimRng {
            let bytes: Vec<_> = hex
                .as_bytes()
                .chunks_exact(2)
                .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
                .collect();
            assert_eq!(bytes.len(), 0x3f4);
            let word = |offset| u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
            SimRng {
                disabled: bytes[0],
                index_a: word(4) as i32,
                index_b: word(8) as i32,
                state: (0..250).map(|i| word(12 + i * 4)).collect(),
            }
        }
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/mapgen_range.json",
        ))
        .unwrap();
        let mut compared = 0;
        for row in corpus["cases"].as_array().unwrap() {
            for call in row["calls"].as_array().unwrap() {
                // The generic native helper's other ranges and disturbed
                // incoming precision/rounding are deliberate negative controls,
                // outside the operation exposed by this Rust API.
                if call["low"] != 0
                    || call["high"] != 3
                    || call["before_control"]["fpcw"].as_u64().unwrap() & 0xf00 != 0xe00
                {
                    continue;
                }
                let mut rng = retained(call["before_state_hex"].as_str().unwrap());
                let after = retained(call["after_state_hex"].as_str().unwrap());
                assert_eq!(
                    u64::from(rng.next_high_two_bits()),
                    call["result"].as_u64().unwrap(),
                    "{}",
                    row["input"]["name"]
                );
                assert_eq!(
                    rng.logical_state(),
                    after.logical_state(),
                    "{}",
                    row["input"]["name"]
                );
                assert_eq!(call["raw_draw_count"], 1);
                assert_eq!(call["state_advance_count"], u64::from(rng.disabled == 0));
                compared += 1;
            }
        }
        assert_eq!(compared, 72, "all startup-mode bridge range requests");
    }

    #[test]
    fn sim_rng_logical_view_and_state_expose_all_250_words_without_mutation() {
        let rng = SimRng::new(0);
        let before = rng.state();
        let view = rng.logical_view();
        let owned = rng.logical_state();
        assert_eq!(view.disabled, 0);
        assert_eq!((view.index_a, view.index_b), (0, 0x67));
        assert_eq!(view.words.len(), 250);
        assert_eq!(owned.words.as_slice(), view.words);
        assert_eq!(rng.state(), before, "observation must not advance the RNG");
    }

    #[test]
    fn seed_zero_logical_state_matches_native_fixture_edges() {
        let state = SimRng::new(0).logical_state();
        assert_eq!(
            &state.words[..4],
            &[0xAD2E_AA18, 0x6670_51BB, 0xBEB8_385D, 0x1293_A6D6]
        );
        assert_eq!(
            &state.words[246..],
            &[0x62FD_BE0F, 0x2034_06B3, 0xAD5E_A053, 0xD72E_1536]
        );
    }

    #[test]
    fn test_rng_repeatable_sequence() {
        let mut a = SimRng::new(12345);
        let mut b = SimRng::new(12345);
        for _ in 0..128 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn test_rng_range_bounds() {
        let mut rng = SimRng::new(1);
        for _ in 0..256 {
            let v = rng.next_range_u32(7);
            assert!(v < 7);
        }
    }

    #[test]
    fn test_inclusive_range_bounds() {
        let mut rng = SimRng::new(42);
        for _ in 0..256 {
            let v = rng.next_range_u32_inclusive(1, 5);
            assert!((1..=5).contains(&v));
        }
    }

    #[test]
    fn test_inclusive_range_degenerate() {
        let mut rng = SimRng::new(1);
        let before = rng.state();
        assert_eq!(rng.next_range_u32_inclusive(7, 7), 7);
        assert_eq!(rng.state(), before);

        let before = rng.state();
        let value = rng.next_range_u32_inclusive(7, 3);
        assert!((3..=7).contains(&value));
        assert_ne!(rng.state(), before);
    }

    #[test]
    fn test_exclusive_single_choice_consumes_no_draw() {
        let mut rng = SimRng::new(99);
        let before = rng.state();
        assert_eq!(rng.next_range_u32(1), 0);
        assert_eq!(rng.state(), before);
    }

    #[test]
    fn test_gamemd_raw_sequence_seed_one() {
        let mut rng = SimRng::new(1);
        assert_eq!(rng.next_u32(), 0x78B7_6ED5);
        assert_eq!(rng.next_u32(), 0x275D_74AE);
        assert_eq!(rng.next_u32(), 0xDA63_B931);
    }

    #[test]
    fn next_raw_abs_modulo_seed_one_golden_and_single_draw() {
        // seed=1 raw stream is 0x78B76ED5, 0x275D74AE, 0xDA63B931
        // (test_gamemd_raw_sequence_seed_one). As i32: +2_025_287_381,
        // +660_436_142, -630_998_735. abs(raw % 80) -> 21, 62, 15.
        let mut rng = SimRng::new(1);
        assert_eq!(rng.next_raw_abs_modulo(80), 21);
        assert_eq!(rng.next_raw_abs_modulo(80), 62);
        assert_eq!(rng.next_raw_abs_modulo(80), 15);

        // Exactly one raw draw per call (no rejection loop): one call leaves the
        // stream where a single next_u32() does — distinguishing raw-modulo
        // (always 1 advance) from the mask-and-reject ranged draw (variable).
        let mut a = SimRng::new(1);
        a.next_raw_abs_modulo(80);
        let mut reference = SimRng::new(1);
        reference.next_u32();
        assert_eq!(a.state(), reference.state());
    }

    #[test]
    fn next_raw_modulo_signed_seed_one_golden_keeps_sign() {
        // Same seed=1 stream; signed remainder % 10 -> 1, 2, -5 (sign follows
        // the dividend; the negative third draw must stay negative — abs would
        // wrongly yield +5).
        let mut rng = SimRng::new(1);
        assert_eq!(rng.next_raw_modulo_signed(10), 1);
        assert_eq!(rng.next_raw_modulo_signed(10), 2);
        assert_eq!(rng.next_raw_modulo_signed(10), -5);

        let mut a = SimRng::new(1);
        a.next_raw_modulo_signed(10);
        let mut reference = SimRng::new(1);
        reference.next_u32();
        assert_eq!(a.state(), reference.state());
    }

    #[test]
    fn raw_modulo_zero_n_returns_zero_and_consumes_no_draw() {
        // n == 0 guard: both helpers return 0 without advancing the cursor
        // (callers guard with .max(1); the helper must not divide-by-zero).
        let mut rng = SimRng::new(1);
        let before = rng.state();
        assert_eq!(rng.next_raw_abs_modulo(0), 0);
        assert_eq!(rng.next_raw_modulo_signed(0), 0);
        assert_eq!(rng.state(), before);
    }

    #[test]
    fn signed_random_ranged_matches_unsigned_form_and_swaps_negative_bounds() {
        let mut signed = SimRng::new(0x5EED_0001);
        let mut unsigned = SimRng::new(0x5EED_0001);
        for _ in 0..64 {
            assert_eq!(
                signed.next_range_i32_inclusive(0, 29) as u32,
                unsigned.next_range_u32_inclusive(0, 29)
            );
        }
        assert_eq!(signed.state(), unsigned.state());

        // `RandomRanged(0, -1)` swaps to `(-1, 0)`: one masked draw, result in {-1, 0}.
        let mut swapped = SimRng::new(0x5EED_0002);
        let mut reference = SimRng::new(0x5EED_0002);
        let value = swapped.next_range_i32_inclusive(0, -1);
        let expected = -1 + (reference.next_u32() & 1) as i32;
        assert_eq!(value, expected);
        assert_eq!(swapped.state(), reference.state());

        // Equal bounds consume nothing.
        let mut equal = SimRng::new(0x5EED_0003);
        let before = equal.state();
        assert_eq!(equal.next_range_i32_inclusive(0, 0), 0);
        assert_eq!(equal.state(), before);
    }

    #[test]
    fn random_ranged_power_of_two_span_matches_gamemd_draw_stream() {
        // Seed=1 raw draws are 0x78B76ED5, 0x275D74AE, 0xDA63B931
        // (pinned by test_gamemd_raw_sequence_seed_one). With span=4 the
        // rejection mask is 7, so masked values are 5, 6, 1 -> reject, reject,
        // accept(1). Correct behavior: returns 1 AND consumes exactly 3 raw
        // draws.
        let mut rng = SimRng::new(1);
        let v = rng.next_range_u32_inclusive(0, 4);
        assert_eq!(
            v, 1,
            "RandomRanged(0,4) on seed 1 must reject 5,6 then accept 1"
        );

        // Pin the exact raw-draw count: a reference advanced exactly 3 times
        // must match. The old span-1 mask (=3) accepts the first draw
        // (0x78B76ED5 & 3 = 1), returning 1 but consuming only ONE draw, so
        // this state assert fails on the old code and passes on the corrected
        // code.
        let mut reference = SimRng::new(1);
        for _ in 0..3 {
            reference.next_u32();
        }
        assert_eq!(
            rng.state(),
            reference.state(),
            "RandomRanged(0,4) must consume exactly 3 raw draws (rejection sampling)"
        );
    }

    #[test]
    fn random_ranged_power_of_two_span_can_return_inclusive_top() {
        // span=4 is a power of two; the inclusive top value 4 must be reachable
        // (impossible with the buggy span-1 mask, which caps output at 3).
        let mut rng = SimRng::new(7);
        let mut saw_top = false;
        for _ in 0..4096 {
            let val = rng.next_range_u32_inclusive(0, 4);
            assert!(val <= 4);
            if val == 4 {
                saw_top = true;
            }
        }
        assert!(
            saw_top,
            "RandomRanged(0,4) must be able to return the inclusive top value 4"
        );
    }

    /// Original Random65C780 / RandomRanged65C7E0 histories execute raw and
    /// signed ranged calls across rejection and cursor-wrap boundaries.
    #[test]
    fn ranged_and_raw_calls_match_native_values_draws_and_complete_state() {
        let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/rmg_oracle/vectors/rng.json",
        ))
        .unwrap();
        let mut compared = 0;
        for history in native["ranged_cases"].as_array().unwrap() {
            let label = history["id"].as_str().unwrap();
            let mut random = SimRng::new(history["seed"].as_u64().unwrap());
            for _ in 0..history["advance_raw"].as_u64().unwrap() {
                let _ = random.next_u32();
            }
            assert_eq!(
                random.native_state_hex(),
                history["initial_state_hex"].as_str().unwrap(),
                "{label}"
            );
            for (index, step) in history["steps"].as_array().unwrap().iter().enumerate() {
                assert_eq!(
                    random.native_state_hex(),
                    step["before_state_hex"].as_str().unwrap(),
                    "{label}/{index}"
                );
                let (actual, trace) = super::trace_draws(|| match step["kind"].as_str().unwrap() {
                    "raw" => i64::from(random.next_u32()),
                    "ranged" => i64::from(random.next_range_i32_inclusive(
                        step["low"].as_i64().unwrap() as i32,
                        step["high"].as_i64().unwrap() as i32,
                    )),
                    kind => panic!("unhandled native draw {kind}"),
                });
                assert_eq!(actual, step["result"].as_i64().unwrap(), "{label}/{index}");
                assert_eq!(
                    trace.len() as u64,
                    step["raw_draw_count"].as_u64().unwrap(),
                    "trace {label}/{index}"
                );
                assert_eq!(
                    trace
                        .iter()
                        .map(|draw| draw["value"].clone())
                        .collect::<Vec<_>>(),
                    *step["raw_draws"].as_array().unwrap(),
                    "raw trace {label}/{index}",
                );
                assert_eq!(
                    random.native_state_hex(),
                    step["after_state_hex"].as_str().unwrap(),
                    "{label}/{index}"
                );
                compared += 1;
            }
        }
        assert_eq!(compared, 98);
    }
}
