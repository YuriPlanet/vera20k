//! The map-generation RNG: the native generator's state and raw draw
//! (`util::native_random`, shared with the scenario stream) with this
//! generator's caller-side range reduction.
//!
//! Verified against golden vectors in `tools/rmg_oracle/vectors/rng.json`,
//! which were produced by running the original routines under emulation.

use super::x87::TruncF64;
use crate::rng_continuation::{MAPGEN_RNG_STATE_WORDS, MapGenRngContinuation};
use crate::util::native_random::{self, LAG};

/// Number of state words in the generator buffer.
const STATE_LEN: usize = MAPGEN_RNG_STATE_WORDS;
/// Multiplier that converts a raw draw to `[0, 1)`.
///
/// This is NOT bit-exact `2^-32`: the stored constant carries one extra
/// mantissa bit. Writing `1.0 / 4294967296.0` here silently diverges from the
/// original on values that land near a rounding boundary.
pub const RANGE_K_BITS: u64 = 0x3DF0_0000_0010_0000;

/// The map generator's random number generator.
#[derive(Debug, Clone)]
pub struct RmgRng {
    state: [u32; STATE_LEN],
    idx_a: usize,
    idx_b: usize,
}

impl RmgRng {
    /// Seed the generator ([`native_random::seeded_words`]).
    pub fn new(seed: u16) -> Self {
        Self {
            state: native_random::seeded_words(u32::from(seed)),
            idx_a: 0,
            idx_b: LAG,
        }
    }

    /// Seal the exact current native cursor into the move-only handoff DTO.
    pub(crate) fn into_continuation(self) -> MapGenRngContinuation {
        MapGenRngContinuation::from_native_parts(0, self.state, self.idx_a, self.idx_b)
    }

    /// Draw one raw word: XOR the lagged pair into the leading slot, return it,
    /// then advance both cursors with wraparound.
    pub fn next_u32(&mut self) -> u32 {
        native_random::draw(&mut self.state, &mut self.idx_a, &mut self.idx_b)
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
