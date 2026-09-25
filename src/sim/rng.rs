//! Deterministic PRNG used by simulation systems.
//!
//! Mirrors gamemd.exe `Random::RandomRanged`/scenario RNG shape for
//! gameplay-visible randomness: one auditable stream, 250-word XOR-lag state,
//! inclusive sorted ranged draws, and rejection sampling.

use std::hash::{Hash, Hasher};

use crate::rng_continuation::MapGenRngContinuation;

/// The `double` at `0x007E3570` that scales a `RandomRanged(0, 0x7ffffffe)`
/// draw onto `[0, 1]` for a unit-interval probability gate (Spark spawns, the
/// `CrewEscape=` roll). Raw bytes `00 00 40 00 00 00 00 3E`, i.e.
/// `1.0 / 2147483646.0` — the reciprocal of the draw's own inclusive top, NOT
/// `2^-31`. The two differ by `2^-30` relative, which is a real bias in a
/// deterministic gate, so this is carried as bits and never recomputed.
pub(crate) const RANDOM_RANGED_UNIT_SCALE: crate::util::native_x87::NativeF64Bits =
    crate::util::native_x87::NativeF64Bits::from_bits(0x3e00_0000_0040_0000);

const RNG_TABLE_LEN: usize = 250;
const RNG_INDEX_B_SEED: i32 = 0x67;
const INIT_TABLE_1: [u32; 4] = [0xBAA9_6887, 0x1E17_D32C, 0x03BC_DC3C, 0x0F33_D1B2];
const INIT_TABLE_2: [u32; 4] = [0x4B0F_3B58, 0xE874_F0C3, 0x6955_C5A6, 0x55A7_CA46];

/// Deterministic simulation RNG.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SimRng {
    disabled: u8,
    index_a: i32,
    index_b: i32,
    state: Vec<u32>,
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
        let mut rng = Self {
            disabled: 0,
            index_a: 0,
            index_b: RNG_INDEX_B_SEED,
            state: vec![0; RNG_TABLE_LEN],
        };
        rng.reseed(seed as u32);
        rng
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

    fn reseed(&mut self, seed: u32) {
        self.index_a = 0;
        self.index_b = RNG_INDEX_B_SEED;

        for (entry_index, slot) in self.state.iter_mut().enumerate() {
            let mut prev = seed;
            let mut mixed = entry_index as u32;
            for i in 0..INIT_TABLE_1.len() {
                let input = INIT_TABLE_1[i] ^ mixed;
                let high = (input as i32) >> 16;
                let low = input & 0xFFFF;
                let square_mix =
                    (!high.wrapping_mul(high)).wrapping_add((low as i32).wrapping_mul(low as i32));
                let rotated = ((square_mix >> 16) as u32) | ((square_mix as u32) << 16);
                let cross = (high as u32).wrapping_mul(low);
                let next = (rotated ^ INIT_TABLE_2[i]).wrapping_add(cross) ^ prev;
                prev = mixed;
                mixed = next;
            }
            *slot = mixed;
        }

        self.disabled = 0;
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
        let value = self.state[a] ^ self.state[b];
        self.state[a] = value;

        self.index_a += 1;
        if self.index_a >= RNG_TABLE_LEN as i32 {
            self.index_a = 0;
        }
        self.index_b += 1;
        if self.index_b >= RNG_TABLE_LEN as i32 {
            self.index_b = 0;
        }

        value
    }

    /// Random integer in [0, max_exclusive). Returns 0 for max_exclusive=0.
    pub fn next_range_u32(&mut self, max_exclusive: u32) -> u32 {
        if max_exclusive == 0 {
            return 0;
        }
        self.next_range_u32_inclusive(0, max_exclusive - 1)
    }

    /// Inclusive ranged draw in gamemd's *scaled* (multiply-high) `RandomRanged`
    /// shape — the variant used by the map-gen / bridge-tile RNG path, distinct
    /// from `next_range_u32_inclusive` (which masks the LOW bits and rejects).
    ///
    /// gamemd computes `lo + ftol(raw · range · (2^-32 + 2^-64))` with the FPU
    /// in truncate-toward-zero mode, then loops while the result exceeds `hi`.
    /// The scale `2^-32 + 2^-64` equals `(2^32 + 1) / 2^64`, so the truncated
    /// product is exactly the integer `(raw · range · (2^32 + 1)) >> 64`, which
    /// is provably in `0..=range-1` for every `raw` (max raw maps to range-1).
    /// The original's rejection branch is therefore unreachable for this exact
    /// integer form and is omitted.
    ///
    /// For the inclusive `(0, 3)` bridge-repair-variant range this reduces to
    /// the high two bits of one draw (`raw >> 30`). Verified bit-identical to
    /// the binary for that range; wider ranges (only reachable from the
    /// random-map generator, which this engine does not run) are not separately
    /// validated against the original's double-precision rounding.
    ///
    /// Unlike `next_range_u32_inclusive`, this consumes one draw even for equal
    /// bounds — matching the binary, which has no equal-bounds early-out here.
    pub fn next_range_u32_inclusive_scaled(&mut self, low: u32, high: u32) -> u32 {
        let (lo, hi) = if low <= high {
            (low, high)
        } else {
            (high, low)
        };
        let range = u64::from(hi - lo) + 1;
        // (2^-32 + 2^-64) == (2^32 + 1) / 2^64
        const SCALE_NUMERATOR: u128 = (1u128 << 32) + 1;
        let raw = u128::from(self.next_u32());
        let scaled = ((raw * u128::from(range) * SCALE_NUMERATOR) >> 64) as u32;
        lo + scaled
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
}
