//! Phobos `TurretAnim.*` (`Ext/BuildingType/Body.cpp:260-265`), the six
//! `Valueable<int>` keys read from a BuildingType's rulesmd.ini section (not
//! ART). Defaults: `Ext/BuildingType/Body.h:232-237`.
//!
//! Consumer: `BuildingClass::AnimationAI` hook `0x451242`
//! (`Ext/Building/Hooks.cpp:1318-1331`) calls `BuildingExt::GetTurretFrame`
//! (`Ext/Building/Body.cpp:488-569`) once per frame and writes the absolute
//! SHP frame into the `BuildingAnimSlot::Turret` (slot 9) animation. The
//! firing block is armed by `BuildingClass::Mission_Attack` hook `0x44B6C7`
//! (`Ext/Building/Hooks.cpp:1333-1357`).
//!
//! ## Dependency rules
//! - Part of rules/ — no dependency on sim/, render/, ui/, etc.

use crate::rules::ini_parser::IniSection;

/// The six `BuildingTypeExt` turret-animation fields, with the constructor
/// defaults `IdleFrames 1`, `LowPowerIdleFrames 0`, `FiringFrames 0`,
/// `LowPowerFiringFrames 0`, `IdleRate 1`, `FiringRate 1`
/// (`Ext/BuildingType/Body.h:232-237`). Phobos' serializer duplicates
/// `FiringFrames` and drops `FiringRate` (`Ext/BuildingType/Body.cpp:482`);
/// VERA reads all six from the type section, not that buggy load path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhobosTurretAnim {
    /// `TurretAnim.IdleFrames=`: frames per facing in the Idle block.
    pub idle_frames: i32,
    /// `TurretAnim.LowPowerIdleFrames=`: frames per facing in LowPowerIdle.
    pub low_power_idle_frames: i32,
    /// `TurretAnim.FiringFrames=`: frames per facing in Firing.
    pub firing_frames: i32,
    /// `TurretAnim.LowPowerFiringFrames=`: frames per facing in LowPowerFiring.
    pub low_power_firing_frames: i32,
    /// `TurretAnim.IdleRate=`: game frames an idle animation frame holds.
    pub idle_rate: i32,
    /// `TurretAnim.FiringRate=`: game frames a firing animation frame holds.
    pub firing_rate: i32,
}

impl Default for PhobosTurretAnim {
    fn default() -> Self {
        Self {
            idle_frames: 1,
            low_power_idle_frames: 0,
            firing_frames: 0,
            low_power_firing_frames: 0,
            idle_rate: 1,
            firing_rate: 1,
        }
    }
}

impl PhobosTurretAnim {
    pub fn from_section(section: &IniSection) -> Self {
        Self {
            idle_frames: section.read_ares_int("TurretAnim.IdleFrames", 1),
            low_power_idle_frames: section.read_ares_int("TurretAnim.LowPowerIdleFrames", 0),
            firing_frames: section.read_ares_int("TurretAnim.FiringFrames", 0),
            low_power_firing_frames: section.read_ares_int("TurretAnim.LowPowerFiringFrames", 0),
            idle_rate: section.read_ares_int("TurretAnim.IdleRate", 1),
            firing_rate: section.read_ares_int("TurretAnim.FiringRate", 1),
        }
    }

    /// `BuildingExt::GetTurretFrame` (`Ext/Building/Body.cpp:488-569`), the
    /// sole call site of hook `0x451242` (`Ext/Building/Hooks.cpp:1318-1331`):
    /// pick the SHP frame block, advance the shared rate tick once and return
    /// the absolute frame `baseOffset + shapeFacing * framesPerFacing + animFrame`.
    ///
    /// `shape_facing` is `ObjectClass::BodyShape[PrimaryFacing.GetValue<5>()]`
    /// (`Body.cpp:492-493`, 0..31); `is_low_power` is
    /// `!StuffEnabled || !IsPowerOnline()` (`Body.cpp:495`).
    pub fn get_turret_frame(
        &self,
        state: &mut TurretAnimState,
        shape_facing: i32,
        is_low_power: bool,
    ) -> i32 {
        let is_firing = state.firing_frame != -1; // Body.cpp:496
        // Four SHP blocks in order Idle, LowPowerIdle, Firing, LowPowerFiring;
        // each block is 32 * Frames (Body.cpp:498-500). Native integer
        // multiplication wraps; extreme authored values must not panic.
        let idle_block = 32_i32.wrapping_mul(self.idle_frames);
        let low_power_idle_block = 32_i32.wrapping_mul(self.low_power_idle_frames);
        let firing_block = 32_i32.wrapping_mul(self.firing_frames);
        let mut frames_per_facing = self.idle_frames;
        let mut base_offset = 0;
        let mut has_firing_frames = false;

        if is_low_power {
            // Body.cpp:506-519.
            if is_firing && self.low_power_firing_frames > 0 {
                frames_per_facing = self.low_power_firing_frames;
                base_offset = idle_block + low_power_idle_block + firing_block;
                has_firing_frames = true;
            } else if self.low_power_idle_frames > 0 {
                frames_per_facing = self.low_power_idle_frames;
                base_offset = idle_block;
            }
        } else if is_firing && self.firing_frames > 0 {
            // Body.cpp:520-528.
            frames_per_facing = self.firing_frames;
            base_offset = idle_block + low_power_idle_block;
            has_firing_frames = true;
        }

        let mut anim_frame = 0;
        if is_firing && has_firing_frames {
            // Body.cpp:532-549. animFrame is the pre-advance frame.
            anim_frame = state.firing_frame;
            state.rate_tick = state.rate_tick.wrapping_add(1);
            if state.rate_tick >= self.firing_rate {
                state.rate_tick = 0;
                state.firing_frame = state.firing_frame.wrapping_add(1);
            }
            if state.firing_frame >= frames_per_facing {
                state.firing_frame = -1;
                state.idle_frame = 0;
                state.rate_tick = 0;
            }
        } else if frames_per_facing > 1 {
            // Body.cpp:550-566: the idle branch advances only with >1 frame.
            anim_frame = state.idle_frame;
            state.rate_tick = state.rate_tick.wrapping_add(1);
            if state.rate_tick >= self.idle_rate {
                state.rate_tick = 0;
                state.idle_frame = state.idle_frame.wrapping_add(1);
            }
            if state.idle_frame >= frames_per_facing {
                state.idle_frame = 0;
                state.rate_tick = 0;
            }
        }
        // Body.cpp:568. Rate <= 0 keeps the `>=` test true every frame.
        base_offset
            .wrapping_add(shape_facing.wrapping_mul(frames_per_facing))
            .wrapping_add(anim_frame)
    }

    /// `BuildingClass_Mission_Attack_TurretAnim` hook `0x44B6C7`
    /// (`Ext/Building/Hooks.cpp:1333-1357`): on the firing instant, arm the
    /// firing block when its frame count is positive and no firing animation
    /// is already playing. `is_low_power` selects `LowPowerFiringFrames`.
    pub fn arm_firing(&self, state: &mut TurretAnimState, is_low_power: bool) {
        let firing_frames = if is_low_power {
            self.low_power_firing_frames
        } else {
            self.firing_frames
        };
        if firing_frames > 0 && state.firing_frame == -1 {
            state.firing_frame = 0;
            state.rate_tick = 0;
        }
    }
}

/// `BuildingExt`'s three turret-anim instance fields
/// (`Ext/Building/Body.h:28-30`, constructor `Body.h:47-49`). Idle and firing
/// share one `rate_tick`; there is no separate firing-rate tick, no
/// `UpdateTypeData` reset (`Body.cpp:603-605` serializes them).
///
/// Presentation state: VERA leaves it out of `world_hash`, as it does the
/// voxel `TurretAnimCounter` (`sim/combat/gattling.rs:528`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TurretAnimState {
    idle_frame: i32,
    firing_frame: i32,
    rate_tick: i32,
}

impl Default for TurretAnimState {
    fn default() -> Self {
        Self {
            idle_frame: 0,
            firing_frame: -1,
            rate_tick: 0,
        }
    }
}

#[cfg(all(test, tests_rules_o_z))]
#[path = "phobos_turret_anim_tests.rs"]
mod tests;
