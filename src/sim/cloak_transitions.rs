//! Native cloak transition entry points and their arg-sensitive sound edge.

use super::{CloakRuntime, CloakStepTimer};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StartCloakingResult {
    pub transitioned: bool,
    pub play_sound: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StartUncloakingResult {
    pub transitioned: bool,
    pub play_sound: bool,
}

impl CloakRuntime {
    /// `TechnoClass::StartCloaking @ 0x00703770`. Native accepts states zero
    /// and three, performs the state/timer writes first, then plays the exact
    /// current coordinate only when the boolean sound-suppression argument is
    /// zero.
    pub(crate) fn start_cloaking(
        &mut self,
        now: i32,
        speed: i32,
        suppress_sound: bool,
    ) -> StartCloakingResult {
        if !matches!(self.state, 0 | 3) {
            return StartCloakingResult {
                transitioned: false,
                play_sound: false,
            };
        }
        self.state = 1;
        self.depth = 0;
        self.step_delta = 1;
        self.step_timer = CloakStepTimer::started(now, speed);
        StartCloakingResult {
            transitioned: true,
            play_sound: !suppress_sound,
        }
    }

    /// `TechnoClass::StartUncloaking @ 0x007036C0`. Native's boolean argument
    /// is a sound-suppression flag: zero plays RulesClass+0x6A0 through
    /// `VocClass::PlayAt @ 0x007509E0`, one performs only the state writes.
    pub(crate) fn start_uncloaking(
        &mut self,
        now: i32,
        speed: i32,
        cloaking_stages: i32,
        suppress_sound: bool,
    ) -> StartUncloakingResult {
        if !matches!(self.state, 1 | 2) {
            return StartUncloakingResult {
                transitioned: false,
                play_sound: false,
            };
        }
        self.state = 3;
        self.depth = cloaking_stages.wrapping_sub(1) as u32;
        self.step_delta = -1;
        self.step_timer = CloakStepTimer::started(now, speed);
        StartUncloakingResult {
            transitioned: true,
            play_sound: !suppress_sound,
        }
    }
}
