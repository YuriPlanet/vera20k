//! Retained ObjectClass fall state, shared by Techno and flaming Anim hosts.

/// Object constructor5F3900 clears +2C/+84/+8D/+8E/+8F. ObjectAI5F3FBC
/// updates the rate after movement and landing callbacks; landing clears only
/// +8D.
#[derive(Debug, Default, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct ObjectFallState {
    rate: i32,
    parachuted: bool,
    /// ObjectClass+88: constructor5F3969 clears it; Paradrop5F5AF1
    /// publishes the new Anim before SetOwnerObject5F5AFE.
    #[serde(default)]
    parachute_animation: Option<u64>,
    falling: bool,
    /// WasFallingDown +8E (constructor `0x005F397B`): only FootClass::AI reads
    /// and writes it ([`Self::observe_falling_edge`]). Not in the world hash.
    #[serde(default)]
    was_falling: bool,
    dropped_as_bomb: bool,
}

impl ObjectFallState {
    pub(crate) fn parachute_animation(&self) -> Option<u64> {
        self.parachute_animation
    }

    pub(crate) fn set_parachute_animation(&mut self, animation: Option<u64>) {
        self.parachute_animation = animation;
    }

    /// ObjectClass::AnimPointerExpired5F6DA0 and PointerExpired's
    /// control-insensitive5F5268 arm clear only the matching held identity.
    pub(crate) fn parachute_animation_expired(&mut self, animation: u64) {
        if self.parachute_animation == Some(animation) {
            self.parachute_animation = None;
        }
    }
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

    /// FootClass::AI `0x004DAB3C..0x004DABC2`: when +8D differs from +8E it
    /// is copied over. True for the start of a DropAsBomb fall (+8F), the only
    /// edge on which `VoiceFalling=` plays (`0x004DAB67`).
    pub(crate) fn observe_falling_edge(&mut self) -> bool {
        if self.falling == self.was_falling {
            return false;
        }
        self.was_falling = self.falling;
        self.falling && self.dropped_as_bomb
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

#[cfg(test)]
mod tests {
    use super::ObjectFallState;

    /// FootClass::AI `0x004DAB3C..0x004DABC2`: only the start of a DropAsBomb
    /// fall reports the VoiceFalling edge, once; landing copies +8D back.
    #[test]
    fn only_the_start_of_a_bomb_fall_reports_the_falling_edge() {
        let mut bomb = ObjectFallState::default();
        bomb.begin_bomb();
        assert!(bomb.observe_falling_edge());
        assert!(!bomb.observe_falling_edge());
        bomb.finish();
        assert!(!bomb.observe_falling_edge());
        assert!(!bomb.was_falling);

        let mut chute = ObjectFallState::default();
        chute.begin_parachute();
        assert!(!chute.observe_falling_edge());
        assert!(chute.was_falling);
    }
}
