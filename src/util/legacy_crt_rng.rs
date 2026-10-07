//! The retail executable's thread-local C runtime random stream.
//!
//! This is separate from Main and Scenario RNG. The app owns the UI thread's
//! lifetime; opening another file dialog must not reset the stream.

#[derive(Debug, Clone)]
pub struct LegacyCrtRng {
    state: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supplied_seeds_match_original_crt_instructions() {
        let vectors: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/storage_oracle/crt_random.json",
        ))
        .unwrap();
        let cases = vectors["cases"].as_array().unwrap();
        assert_eq!(cases.len(), 7);
        for (case, seed) in
            cases
                .iter()
                .zip([0, 1, 0x7fff, 0x8000, 0x7fff_ffff, 0x8000_0000, 0xffff_ffff])
        {
            assert_eq!(case["seed"].as_u64().unwrap(), u64::from(seed));
            let mut rng = LegacyCrtRng::with_seed(seed);
            let draws = case["draws"].as_array().unwrap();
            assert_eq!(draws.len(), 32);
            for draw in draws {
                assert_eq!(u64::from(rng.draw15()), draw["value"].as_u64().unwrap());
                assert_eq!(u64::from(rng.state), draw["state"].as_u64().unwrap());
            }
            rng.seed(seed);
            assert_eq!(u64::from(rng.draw15()), draws[0]["value"].as_u64().unwrap());
        }
    }
}

impl Default for LegacyCrtRng {
    fn default() -> Self {
        // CRT thread-context initializer 0x007D13F8, called by startup and by
        // lazy context allocation. This does not prove the first Save sees 1:
        // other active consumers can have drawn or reseeded before it.
        Self::with_seed(1)
    }
}

impl LegacyCrtRng {
    pub const fn with_seed(state: u32) -> Self {
        Self { state }
    }

    /// `_srand` 0x007CB49D replaces context+0x14 without drawing.
    pub fn seed(&mut self, state: u32) {
        self.state = state;
    }

    /// `_rand` 0x007CB4AA, arithmetic at 0x007CB4AF..0x007CB4CB.
    pub fn draw15(&mut self) -> u16 {
        self.state = self.state.wrapping_mul(0x343fd).wrapping_add(0x269ec3);
        ((self.state >> 16) & 0x7fff) as u16
    }
}
