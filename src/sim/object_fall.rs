//! Retained ObjectClass fall state, shared by Techno and flaming Anim hosts.

/// Object constructor5F3900 clears +2C/+84/+8D/+8F. ObjectAI5F3FBC updates
/// the rate after movement and landing callbacks; landing clears only +8D.
#[derive(Debug, Default, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct ObjectFallState {
    rate: i32,
    parachuted: bool,
    falling: bool,
    dropped_as_bomb: bool,
}

impl ObjectFallState {
    pub(crate) fn falling(&self) -> bool {
        self.falling
    }
    pub(crate) fn rate(&self) -> i32 {
        self.rate
    }
    pub(crate) fn parachuted(&self) -> bool {
        self.parachuted
    }
    pub(crate) fn dropped_as_bomb(&self) -> bool {
        self.dropped_as_bomb
    }

    pub(crate) fn begin_parachute(&mut self) {
        self.parachuted = true;
        self.falling = true;
        self.rate = 0;
    }

    pub(crate) fn begin_bomb(&mut self) {
        self.falling = true;
        self.dropped_as_bomb = true;
    }

    /// FlamingGuy425BFC writes +8D without resetting any other fall state.
    pub(crate) fn begin_fall(&mut self) {
        self.falling = true;
    }
    pub(crate) fn finish(&mut self) {
        self.falling = false;
    }

    pub(crate) fn advance_rate(&mut self, parachute_cap: i32, free_fall_cap: i32) {
        self.rate = if self.parachuted {
            self.rate.wrapping_sub(1).max(parachute_cap)
        } else {
            use crate::util::native_x87::{NativeF64Bits, X87Chop53};
            let gravity = X87Chop53::load_f64(NativeF64Bits::from_bits(0x3ff6_6666_6666_6666))
                .expect("finite ObjectAI free-fall gravity 7EF248");
            X87Chop53::ftol_i32_low_masked(X87Chop53::sub(X87Chop53::load_i32(self.rate), gravity))
                .max(free_fall_cap)
        };
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_falling_for_test(&mut self, falling: bool) {
        self.falling = falling;
        self.parachuted = falling;
        self.rate = 0;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_rate_for_test(&mut self, rate: i32) {
        self.rate = rate;
    }
}
