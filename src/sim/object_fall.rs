//! Retained ObjectClass fall state, shared by Techno and flaming Anim hosts.

/// Object constructor5F3900 clears +2C/+84/+8D/+8E/+8F. ObjectAI5F3FBC
/// updates the rate after movement and landing callbacks; landing clears only
/// +8D.
#[derive(Debug, Default, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct ObjectFallState {
    rate: i32,
    /// Object+84 is the retained attached-Anim flag, consumed by ObjectAI5F3FB2.
    /// Retail gamemd.exe SetOwnerObject424B50 sets it and the last normal
    /// detach clears it; PointerExpired425150 can leave it set without an Anim.
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
    pub(crate) fn set_attached_anim_flag(&mut self, attached: bool) {
        self.parachuted = attached;
    }
    pub(crate) fn dropped_as_bomb(&self) -> bool {
        self.dropped_as_bomb
    }

    pub(crate) fn begin_bomb(&mut self) {
        self.falling = true;
        self.dropped_as_bomb = true;
    }

    /// Ares 3.0 ObjectClass_SpawnParachuted_Animation (10047C60, hook
    /// 0x5F5ADD) raises +8F alone for an `IsBomb` Foot's paradrop.
    #[cfg(feature = "ares")]
    pub(crate) fn mark_bomb(&mut self) {
        self.dropped_as_bomb = true;
    }

    /// Ares 3.0 LogicClass_Update_1000 (100680F0, hook 0x55AFB3) under
    /// CanMakeStuffUp: FallRate +2C becomes 0 and +8F rises.
    #[cfg(feature = "ares")]
    pub(crate) fn drop_parachute_as_bomb(&mut self) {
        self.rate = 0;
        self.dropped_as_bomb = true;
    }

    /// Paradrop5F5965 and FlamingGuy425BFC write +8D without resetting the
    /// retained rate or attached-Anim flag.
    pub(crate) fn begin_fall(&mut self) {
        self.falling = true;
    }
    pub(crate) fn finish(&mut self) {
        self.falling = false;
    }

    /// Whether [`Self::observe_falling_edge`] copies +8D over +8E: they differ.
    pub(crate) fn falling_edge_pending(&self) -> bool {
        self.falling != self.was_falling
    }

    /// FootClass::AI `0x004DAB3C..0x004DABC2`: when +8D differs from +8E it
    /// is copied over. True for the start of a DropAsBomb fall (+8F), the only
    /// edge on which `VoiceFalling=` plays (`0x004DAB67`).
    pub(crate) fn observe_falling_edge(&mut self) -> bool {
        if !self.falling_edge_pending() {
            return false;
        }
        self.was_falling = self.falling;
        self.falling && self.dropped_as_bomb
    }

    /// ObjectAI5F3FB2..5F3FFA for an object without a TechnoType: the
    /// attached-Anim flag selects DEC or the 1.4 free-fall gravity.
    ///
    /// Ares 3.0 ObjectClass_Update_MaxFallRate (10049BA0, hook 0x5F3FB2)
    /// steps such an object by 1 either way.
    pub(crate) fn advance_rate(&mut self, parachute_cap: i32, free_fall_cap: i32) {
        #[cfg(feature = "ares")]
        {
            let cap = if self.parachuted {
                parachute_cap
            } else {
                free_fall_cap
            };
            self.rate = self.rate.wrapping_sub(1).max(cap);
        }
        #[cfg(not(feature = "ares"))]
        self.advance_stock_rate(parachute_cap, free_fall_cap);
    }

    /// Ares 3.0 10049BA0 (hook 0x5F3FB2) for a Techno: the Parachute pointer
    /// (+0x88) in place of +84 selects the type's `FallRate.*` step and cap
    /// (`rules_caps` the Rules `ParachuteMaxFallRate` and
    /// `NoParachuteMaxFallRate`), `max(cap, FallRate - step)`.
    #[cfg(feature = "ares")]
    pub(crate) fn advance_techno_rate(
        &mut self,
        fall: &crate::rules::ares_techno_type::FallRateTypeRules,
        rules_caps: [i32; 2],
    ) {
        let (step, cap) = fall.step(self.parachute_animation.is_some(), rules_caps);
        self.rate = self.rate.wrapping_sub(step).max(cap);
    }

    #[cfg(not(feature = "ares"))]
    fn advance_stock_rate(&mut self, parachute_cap: i32, free_fall_cap: i32) {
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

    /// Ares 3.0 ObjectClass_Update_MaxFallRate (10049BA0, hook 0x5F3FB2): a
    /// Techno falls by its `FallRate.NoParachute` to its own cap, and with a
    /// Parachute pointer (not the retained +84 alone) by `FallRate.Parachute`
    /// to the Rules cap; an object without a TechnoType steps 1 either way.
    #[cfg(feature = "ares")]
    #[test]
    fn ares_fall_rate_steps_by_type_and_parachute_pointer() {
        use crate::rules::ini_parser::IniFile;
        let rules = crate::rules::ruleset::RuleSet::from_ini(&IniFile::from_str(
            "[General]\nParachuteMaxFallRate=-3\nNoParachuteMaxFallRate=-100\n\
             [VehicleTypes]\n0=DROP\n[DROP]\nFallRate.NoParachute=3\n\
             FallRate.NoParachuteMax=-7\nFallRate.Parachute=2\n",
        ))
        .unwrap();
        let fall = rules.object("DROP").unwrap().techno_combat().fall_rate();
        let caps = [
            rules.general.parachute_max_fall_rate,
            rules.general.no_parachute_max_fall_rate,
        ];
        let mut techno = ObjectFallState::default();
        techno.set_attached_anim_flag(true);
        let rates: Vec<i32> = (0..4)
            .map(|_| {
                techno.advance_techno_rate(fall, caps);
                techno.rate()
            })
            .collect();
        assert_eq!(rates, [-3, -6, -7, -7]);
        techno.set_parachute_animation(Some(7));
        techno.set_rate_for_test(0);
        techno.advance_techno_rate(fall, caps);
        techno.advance_techno_rate(fall, caps);
        assert_eq!(techno.rate(), -3);
        let mut anim = ObjectFallState::default();
        anim.advance_rate(caps[0], caps[1]);
        anim.advance_rate(caps[0], caps[1]);
        assert_eq!(anim.rate(), -2, "no 1.4 gravity");
    }

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
        chute.begin_fall();
        assert!(!chute.observe_falling_edge());
        assert!(chute.was_falling);
    }
}
