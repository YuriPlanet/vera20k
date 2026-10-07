//! Exact reproduction of the map-generation RNG: a hash-seeded, 250-dword
//! lag-103 XOR generator with caller-side range reduction.
//!
//! Deliberately separate from `sim::rng::SimRng`. That one drives match
//! simulation; this one reproduces a different machine, byte for byte, and
//! sharing code between them would let drift in either corrupt the other.
//!
//! Verified against golden vectors in `tools/rmg_oracle/vectors/rng.json`,
//! which were produced by running the original routines under emulation.

use super::x87::TruncF64;
use crate::rng_continuation::{MAPGEN_RNG_STATE_WORDS, MapGenRngContinuation};

/// Number of state words in the generator buffer.
const STATE_LEN: usize = MAPGEN_RNG_STATE_WORDS;
/// Distance between the two cursors; also the second cursor's start position.
const LAG: usize = 0x67;
/// Seed-hash table 1, consumed at indices 0..3 (one per hash round).
const TABLE1: [u32; 4] = [0xBAA9_6887, 0x1E17_D32C, 0x03BC_DC3C, 0x0F33_D1B2];
/// Seed-hash table 2. The original pre-increments its index before the load,
/// so only entries 1..=4 are ever consumed; entry 0 is kept for provenance.
const TABLE2: [u32; 5] = [
    0x48AA_D7E4,
    0x4B0F_3B58,
    0xE874_F0C3,
    0x6955_C5A6,
    0x55A7_CA46,
];
/// Multiplier that converts a raw draw to `[0, 1)`.
///
/// This is NOT bit-exact `2^-32`: the stored constant carries one extra
/// mantissa bit. Writing `1.0 / 4294967296.0` here silently diverges from the
/// original on values that land near a rounding boundary.
pub const RANGE_K_BITS: u64 = 0x3DF0_0000_0010_0000;

/// Hash rounds applied per seeded state word.
const HASH_ROUNDS: usize = 4;

/// The map generator's random number generator.
#[derive(Debug, Clone)]
pub struct RmgRng {
    state: [u32; STATE_LEN],
    idx_a: usize,
    idx_b: usize,
}

impl RmgRng {
    /// Seed the generator. Each of the 250 state words is produced by four
    /// hash rounds that carry the previous round's pre-mangle value forward.
    pub fn new(seed: u16) -> Self {
        let seed = u32::from(seed);
        let mut state = [0u32; STATE_LEN];
        let mut counter: u32 = 0;

        for slot in state.iter_mut() {
            let mut value = counter;
            counter = counter.wrapping_add(1);
            // Round 0 mixes in the seed; later rounds mix in the previous
            // round's input instead.
            let mut carry = seed;

            for round in 0..HASH_ROUNDS {
                let mangled = TABLE1[round] ^ value;
                let previous = value;

                // Split into signed halves: the original uses arithmetic
                // shifts, so the high half keeps its sign.
                let hi = (mangled as i32) >> 16;
                let lo = (mangled & 0xFFFF) as i32;

                let hi_hi = !hi.wrapping_mul(hi) as u32;
                let hi_lo = hi.wrapping_mul(lo) as u32;
                let lo_lo = lo.wrapping_mul(lo) as u32;

                let sum = hi_hi.wrapping_add(lo_lo);
                // Swap the halves of `sum`, keeping the arithmetic shift.
                let swapped = ((sum as i32 >> 16) as u32) | (sum << 16);

                value = (swapped ^ TABLE2[round + 1]).wrapping_add(hi_lo) ^ carry;
                carry = previous;
            }

            *slot = value;
        }

        Self {
            state,
            idx_a: 0,
            idx_b: LAG,
        }
    }

    /// Seal the exact current native cursor into the move-only handoff DTO.
    pub(crate) fn into_continuation(self) -> MapGenRngContinuation {
        MapGenRngContinuation::from_native_parts(self.state, self.idx_a, self.idx_b)
    }

    /// Draw one raw word: XOR the lagged pair into the leading slot, return it,
    /// then advance both cursors with wraparound.
    pub fn next_u32(&mut self) -> u32 {
        let value = self.state[self.idx_a] ^ self.state[self.idx_b];
        self.state[self.idx_a] = value;

        self.idx_a += 1;
        self.idx_b += 1;
        if self.idx_a >= STATE_LEN {
            self.idx_a = 0;
        }
        if self.idx_b >= STATE_LEN {
            self.idx_b = 0;
        }

        value
    }

    /// Convert a draw to `[0, 1)` using the original's exact constant.
    ///
    /// The multiply truncates: the original loads the draw as an integer and
    /// multiplies under round-toward-zero, so an ordinary `f64` product rounds
    /// the wrong way and drifts by an ulp on some draws.
    pub fn next_unit(&mut self) -> f64 {
        let draw = TruncF64::from_f64(f64::from(self.next_u32()));
        draw.mul(TruncF64::from_f64(f64::from_bits(RANGE_K_BITS)))
            .to_f64()
    }

    /// Inclusive uniform integer in `[min, max]`.
    ///
    /// Operand order matters and is not the obvious one: the original computes
    /// `draw * span * K + min`, scaling by the span *before* converting to a
    /// unit interval. Doing `next_unit() * span` instead rounds differently.
    ///
    /// The rejection loop is part of the behaviour, not a safety net: when the
    /// scaled value lands above `max` the original re-draws, consuming another
    /// word. Anything that reproduces the draw *stream* has to re-draw too.
    pub fn uniform(&mut self, min: i32, max: i32) -> i32 {
        debug_assert!(min <= max, "uniform range must be non-empty");
        let span = TruncF64::from_f64(f64::from(max - min + 1));
        let scale = TruncF64::from_f64(f64::from_bits(RANGE_K_BITS));
        let floor = TruncF64::from_f64(f64::from(min));
        loop {
            let draw = TruncF64::from_f64(f64::from(self.next_u32()));
            let scaled = draw.mul(span).mul(scale).add(floor);
            let value = super::x87::ftol(scaled.to_f64());
            if value <= max {
                return value;
            }
        }
    }

    /// Cursor positions, for tests that assert the seeded starting state.
    #[cfg(test)]
    fn cursors(&self) -> (usize, usize) {
        (self.idx_a, self.idx_b)
    }

    /// State word, for tests that compare against the golden vectors.
    #[cfg(test)]
    fn state_word(&self, index: usize) -> u32 {
        self.state[index]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Golden vectors captured from the original binary under emulation.
    fn vectors() -> serde_json::Value {
        let doc: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/rmg_oracle/vectors/rng.json",
        ))
        .unwrap();
        assert_eq!(
            doc["source"].as_str(),
            Some("unicorn/gamemd.exe"),
            "vectors must be machine-derived, never hand-written"
        );
        doc
    }

    fn hex_bytes(text: &str) -> Vec<u8> {
        (0..text.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
            .collect()
    }

    #[test]
    fn range_constant_is_not_two_pow_minus_32() {
        assert_eq!(RANGE_K_BITS, 0x3DF0_0000_0010_0000);
        assert_ne!(
            RANGE_K_BITS,
            (1.0f64 / 4294967296.0).to_bits(),
            "the original constant carries an extra mantissa bit"
        );
    }

    #[test]
    fn seeded_state_matches_golden_vectors() {
        let doc = vectors();
        let cases = doc["cases"].as_array().unwrap();
        assert!(!cases.is_empty(), "vector file has no cases");

        for case in cases {
            let seed = case["seed"].as_u64().unwrap() as u16;
            let rng = RmgRng::new(seed);

            assert_eq!(case["locked"].as_u64(), Some(0), "seed {seed}: locked flag");
            assert_eq!(
                rng.cursors(),
                (
                    case["idx_a"].as_u64().unwrap() as usize,
                    case["idx_b"].as_u64().unwrap() as usize
                ),
                "seed {seed}: cursor start positions"
            );

            let raw = hex_bytes(case["state_hex"].as_str().unwrap());
            assert_eq!(raw.len(), STATE_LEN * 4, "seed {seed}: state length");
            for (index, chunk) in raw.chunks_exact(4).enumerate() {
                let expected = u32::from_le_bytes(chunk.try_into().unwrap());
                assert_eq!(
                    rng.state_word(index),
                    expected,
                    "seed {seed}: state word {index}"
                );
            }
        }
    }

    #[test]
    fn draw_stream_matches_golden_vectors() {
        let doc = vectors();
        for case in doc["cases"].as_array().unwrap() {
            let seed = case["seed"].as_u64().unwrap() as u16;
            let mut rng = RmgRng::new(seed);

            let draws = case["draws"].as_array().unwrap();
            assert!(!draws.is_empty(), "seed {seed}: no draws recorded");
            for (index, expected) in draws.iter().enumerate() {
                let expected = u32::from_str_radix(expected.as_str().unwrap(), 16).unwrap();
                assert_eq!(rng.next_u32(), expected, "seed {seed}: draw {index}");
            }
        }
    }

    #[test]
    fn uniform_stays_within_inclusive_bounds() {
        let mut rng = RmgRng::new(1234);
        for _ in 0..2000 {
            let value = rng.uniform(2, 8);
            assert!((2..=8).contains(&value), "uniform produced {value}");
        }
    }

    #[test]
    fn uniform_reaches_both_endpoints() {
        let mut rng = RmgRng::new(4321);
        let mut low = false;
        let mut high = false;
        for _ in 0..4000 {
            match rng.uniform(0, 3) {
                0 => low = true,
                3 => high = true,
                _ => {}
            }
        }
        assert!(low && high, "inclusive range must reach 0 and 3");
    }

    #[test]
    fn next_unit_is_in_unit_interval() {
        let mut rng = RmgRng::new(7);
        for _ in 0..2000 {
            let value = rng.next_unit();
            assert!((0.0..1.0).contains(&value), "next_unit produced {value}");
        }
    }

    #[test]
    fn cursors_wrap_without_desync() {
        // Run well past one full lap of the 250-word buffer.
        let mut rng = RmgRng::new(99);
        for _ in 0..1000 {
            rng.next_u32();
        }
        let (a, b) = rng.cursors();
        assert!(a < STATE_LEN && b < STATE_LEN, "cursors left the buffer");
        assert_eq!(
            (b + STATE_LEN - a) % STATE_LEN,
            LAG,
            "cursors must stay exactly LAG apart"
        );
    }
}
