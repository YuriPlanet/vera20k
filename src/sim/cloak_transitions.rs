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
    pub(super) fn start_cloaking(
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
    pub(super) fn start_uncloaking(
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

    /// Virtual `StartCloaking +0x460 @ 0x00703770` reached from the active
    /// sensor-count resident callback `0x006F4EB0` with arg zero.
    pub(crate) fn start_cloaking_from_sensor(
        &mut self,
        now: i32,
        speed: i32,
    ) -> StartCloakingResult {
        self.start_cloaking(now, speed, false)
    }

    /// `TechnoClass::ReceiveDamage @ 0x0070281D` invokes virtual `+0xFC`
    /// (`0x00703850`, a plain `StartUncloaking(0)` wrapper) for every damage
    /// result that is not NowDead — the heal case included, because the
    /// `if (damage < 0) return` early-out sits AFTER this call. Argument zero,
    /// so the transition owns a `CloakSound`.
    pub(crate) fn start_uncloaking_from_damage(
        &mut self,
        now: i32,
        speed: i32,
        cloaking_stages: i32,
    ) -> StartUncloakingResult {
        self.start_uncloaking(now, speed, cloaking_stages, false)
    }

    /// `FootClass::PerCellProcess @ 0x004D8829` invokes the same `+0xFC`
    /// wrapper when a fully cloaked mover enters a cell one of whose eight
    /// neighbours holds a non-allied `Sensors=yes` object.
    pub(crate) fn start_uncloaking_from_sensor_neighbour(
        &mut self,
        now: i32,
        speed: i32,
        cloaking_stages: i32,
    ) -> StartUncloakingResult {
        self.start_uncloaking(now, speed, cloaking_stages, false)
    }

    /// Cell483480 calls the ground-list receiver+FC (Techno703850), then
    /// loads its next link. Walk75BBAE and Drive/Ship code1 share this owner;
    /// the transition, cadence and sound argument are the ordinary7036C0 body.
    pub(crate) fn start_uncloaking_from_mover_contact(
        &mut self,
        now: i32,
        speed: i32,
        cloaking_stages: i32,
    ) -> StartUncloakingResult {
        self.start_uncloaking(now, speed, cloaking_stages, false)
    }

    /// `UnitClass::Fire_At_Target @ 0x00736DF0` case 9 invokes virtual
    /// `StartUncloaking +0x45C @ 0x007036C0` after rechecking CanFireAt.
    pub(crate) fn start_uncloaking_to_fire(
        &mut self,
        now: i32,
        speed: i32,
        cloaking_stages: i32,
    ) -> StartUncloakingResult {
        self.start_uncloaking(now, speed, cloaking_stages, false)
    }
}
