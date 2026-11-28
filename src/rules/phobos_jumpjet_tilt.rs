//! Phobos `JumpjetTilt.*` (`Ext/UnitType/Body.cpp:63-67`).
//!
//! A jumpjet body is drawn through `JumpjetLocomotionClass_Draw_Matrix`
//! (vtable slot `0x7ECD8C`, replaced by
//! `Ext/TechnoType/Hooks.MatrixOp.cpp:391-523`). When the stock
//! `TiltCrashJumpjet=` arm has no rocking angle, Phobos derives a draw-only
//! pitch (forwards) and roll (sideways) from the locomotor's current speed,
//! acceleration and turn difference, then rotates the body by them. No
//! simulation field is written.
//!
//! Formula (`Ext/TechnoType/Hooks.MatrixOp.cpp:391-399, 447-491`):
//!
//! ```text
//! forwards = clamp((accel * fwd_accel + speed * fwd_speed) * base_forwards,
//!                  -max, max)
//! sideways = clamp(speed * side_speed * (i16)facing_difference * side_rotation
//!                  * base_sideways, -max, max)   // only while turning
//! base_forwards = (pi/2 / 4) / 32
//! base_sideways = (pi/2 / 4) / (32768 * 32)
//! ```
//!
//! Each result becomes a `DirStruct` (YRpp `Dir.h:14-96`), which quantizes it
//! to one of 128 facings; the draw uses that quantized radian and skips a zero
//! axis. The double-to-DirStruct conversion truncates towards zero before the
//! 7-bit rounding, so an exact positive half step rounds down while a negative
//! one rounds up (away from zero).
//!
//! ## Residuals
//!
//! The shadow arm (`UnitClass_DrawAsVXL_Shadow` hook `0x73C47A`,
//! `Ext/TechnoType/Hooks.MatrixOp.cpp:729-773`) now reuses the body's
//! already-resolved `JumpjetTilt` pose: the lifted shadow pose/part/pose-key
//! carry the same roll/pitch, and the shadow matrix arm reuses the crash
//! rotation product. The shadow keeps its own `Shadow_Point` anchor; the other
//! locomotor poses (`Jumpjet` crash rocking, `FlyNormal`, `Rocket`) keep the
//! stock shadow policy.
//!
//! Still visual-only: the frame-local shadow template is still sampled from
//! the baked untilted stencil — no runtime caller yet hands the shadow render
//! params the tilted `body_tilt`, so the Fresh/Retained template is not
//! reprojected frame-by-frame through the tilt matrix, and the high-flight
//! unmasked entry is not tilted. The body draw and the simulation are
//! unaffected.
//!
//! ## Keys
//!
//! The five globals are `Valueable`: `[AudioVisual] JumpjetTilt` (default
//! `no`) and the four factors (default `1.0`), constructs
//! `Ext/Rules/Body.h:646-650`, read `Ext/Rules/Body.cpp:203-207`. The five
//! per-type fields are `Nullable`: an unset value falls back to the global
//! (`Ext/UnitType/Body.h:48-52`, constructs `:112-116`), read
//! `Ext/UnitType/Body.cpp:63-67`. Only the factors are `JumpjetTilt.*` keys;
//! the bare boolean is the gate the same consumer passes through
//! `Valueable::Get(global)`.
//!
//! ## Dependency rules
//! - Part of rules/ — no dependencies on sim/, render/, ui/, etc.

use crate::rules::ini_parser::{IniFile, IniSection};

/// `Math::HalfPi` (`YRpp/YRMath.h:17`) as the double Phobos stores its
/// `constexpr auto` constants from.
const HALF_PI: f64 = std::f64::consts::FRAC_PI_2;
const BASE_TILT: f64 = HALF_PI / 4.0;
const BASE_SPEED: f64 = 32.0;
const BASE_TURN_RAW: f64 = 32768.0;
/// `MaxTilt`: `static_cast<float>(Math::HalfPi)`.
const MAX_TILT: f32 = HALF_PI as f32;
const FORWARD_BASE_TILT: f64 = BASE_TILT / BASE_SPEED;
const SIDEWAYS_BASE_TILT: f64 = BASE_TILT / (BASE_TURN_RAW * BASE_SPEED);

/// `RulesExt`'s five `[AudioVisual]` JumpjetTilt fields
/// (`Ext/Rules/Body.h:109-113`), construct defaults `false` and `1.0`
/// (`:646-650`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PhobosJumpjetTiltGlobal {
    /// `JumpjetTilt=`: the default a type's own nullable field inherits.
    pub enabled: bool,
    pub forward_accel_factor: f64,
    pub forward_speed_factor: f64,
    pub sideways_rotation_factor: f64,
    pub sideways_speed_factor: f64,
}

impl Default for PhobosJumpjetTiltGlobal {
    fn default() -> Self {
        Self {
            enabled: false,
            forward_accel_factor: 1.0,
            forward_speed_factor: 1.0,
            sideways_rotation_factor: 1.0,
            sideways_speed_factor: 1.0,
        }
    }
}

impl PhobosJumpjetTiltGlobal {
    /// `RulesExt::LoadFromINI` `Ext/Rules/Body.cpp:203-207`. `Valueable`
    /// reads keep the current value on an absent, empty or unparsable token.
    pub fn from_ini(ini: &IniFile) -> Self {
        Self::from_section(&ini.section_or_empty("AudioVisual"))
    }

    pub fn from_section(section: &IniSection) -> Self {
        let defaults = Self::default();
        Self {
            enabled: section.read_bool("JumpjetTilt", defaults.enabled),
            forward_accel_factor: section.read_phobos_double(
                "JumpjetTilt.ForwardAccelFactor",
                defaults.forward_accel_factor,
            ),
            forward_speed_factor: section.read_phobos_double(
                "JumpjetTilt.ForwardSpeedFactor",
                defaults.forward_speed_factor,
            ),
            sideways_rotation_factor: section.read_phobos_double(
                "JumpjetTilt.SidewaysRotationFactor",
                defaults.sideways_rotation_factor,
            ),
            sideways_speed_factor: section.read_phobos_double(
                "JumpjetTilt.SidewaysSpeedFactor",
                defaults.sideways_speed_factor,
            ),
        }
    }
}

/// The five `UnitTypeExt` JumpjetTilt fields (`Ext/UnitType/Body.h:48-52`),
/// all `Nullable` so `None` inherits [`PhobosJumpjetTiltGlobal`].
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PhobosJumpjetTiltType {
    pub enabled: Option<bool>,
    pub forward_accel_factor: Option<f64>,
    pub forward_speed_factor: Option<f64>,
    pub sideways_rotation_factor: Option<f64>,
    pub sideways_speed_factor: Option<f64>,
}

impl PhobosJumpjetTiltType {
    /// `UnitTypeExt::LoadFromINIFile` `Ext/UnitType/Body.cpp:63-67`.
    pub fn from_section(section: &IniSection) -> Self {
        Self {
            enabled: section.read_phobos_nullable_bool("JumpjetTilt", None),
            forward_accel_factor: section
                .read_phobos_nullable_double("JumpjetTilt.ForwardAccelFactor", None),
            forward_speed_factor: section
                .read_phobos_nullable_double("JumpjetTilt.ForwardSpeedFactor", None),
            sideways_rotation_factor: section
                .read_phobos_nullable_double("JumpjetTilt.SidewaysRotationFactor", None),
            sideways_speed_factor: section
                .read_phobos_nullable_double("JumpjetTilt.SidewaysSpeedFactor", None),
        }
    }

    /// [`Valueable::Get`] fallback: `None` when neither the type nor the
    /// global enables the tilt; otherwise the four resolved factors.
    pub fn resolve(&self, global: &PhobosJumpjetTiltGlobal) -> Option<ResolvedJumpjetTilt> {
        self.enabled
            .unwrap_or(global.enabled)
            .then_some(ResolvedJumpjetTilt {
                forward_accel_factor: self
                    .forward_accel_factor
                    .unwrap_or(global.forward_accel_factor),
                forward_speed_factor: self
                    .forward_speed_factor
                    .unwrap_or(global.forward_speed_factor),
                sideways_rotation_factor: self
                    .sideways_rotation_factor
                    .unwrap_or(global.sideways_rotation_factor),
                sideways_speed_factor: self
                    .sideways_speed_factor
                    .unwrap_or(global.sideways_speed_factor),
            })
    }
}

/// The enabled type's four factors after the global fallback.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResolvedJumpjetTilt {
    pub forward_accel_factor: f64,
    pub forward_speed_factor: f64,
    pub sideways_rotation_factor: f64,
    pub sideways_speed_factor: f64,
}

impl ResolvedJumpjetTilt {
    /// The airborne tilt the Draw_Matrix default arm applies, as
    /// `[sideways, forwards]` radians quantized to the 128 `DirStruct`
    /// facings. `None` when both quantized angles are zero, which is the
    /// native plain-matrix path.
    ///
    /// `current_speed` is the locomotor's binary64 CurrentSpeed, `accel` its
    /// binary32 `JumpjetAccel`, `difference_raw` the signed
    /// `FacingClass::Difference().Raw` (destination minus rotation start).
    pub fn tilt_angles(
        &self,
        current_speed: f64,
        accel: f32,
        rotating: bool,
        difference_raw: i16,
    ) -> Option<[f32; 2]> {
        // The native expressions evaluate each double product to float before
        // the float add/multiply and the final double product.
        let forward_speed = (current_speed * self.forward_speed_factor) as f32;
        let forward_accel = (f64::from(accel) * self.forward_accel_factor) as f32;
        let forwards =
            clamp_tilt((f64::from(forward_accel + forward_speed) * FORWARD_BASE_TILT) as f32);

        let sideways = if rotating {
            let side_speed = (current_speed * self.sideways_speed_factor) as f32;
            let side_rotation = (f64::from(difference_raw) * self.sideways_rotation_factor) as f32;
            clamp_tilt((f64::from(side_speed * side_rotation) * SIDEWAYS_BASE_TILT) as f32)
        } else {
            0.0
        };

        if dir_facing_128(sideways) == 0 && dir_facing_128(forwards) == 0 {
            return None;
        }
        Some([dir_radian_128(sideways), dir_radian_128(forwards)])
    }
}

/// `Math::clamp` (`YRpp/YRMath.h:76-84`): NaN compares false and passes.
fn clamp_tilt(value: f32) -> f32 {
    if value < -MAX_TILT {
        -MAX_TILT
    } else if MAX_TILT < value {
        MAX_TILT
    } else {
        value
    }
}

/// `DirStruct(double)` (`YRpp/Dir.h:16, 86-96`): truncate the radian to a
/// 16-bit raw word. The 90-degree bias is removed again by the 7-bit readers.
fn dir_raw(radian: f32) -> u16 {
    let dir = (f64::from(radian) / (-std::f64::consts::TAU / 65536.0)) as i32;
    (dir.wrapping_add(16384) & 0xFFFF) as u16
}

/// `GetValue<7>(0)` (`YRpp/Dir.h:44-47, 100-111`): round the raw word to 128
/// facings, half up.
fn dir_value_128(radian: f32) -> u32 {
    (((u32::from(dir_raw(radian)) >> 8) + 1) >> 1) & 127
}

/// `GetFacing<128>(96)` (`YRpp/Dir.h:55-62`): the cache-index spelling, with
/// the value rotated back so a zero angle keeps facing 0.
fn dir_facing_128(radian: f32) -> u32 {
    (dir_value_128(radian) + 96) & 127
}

/// `GetRadian<128>()` (`YRpp/Dir.h:73-83`): the quantized radian the draw
/// rotates by.
fn dir_radian_128(radian: f32) -> f32 {
    let steps = dir_value_128(radian) as f64 - 32.0;
    (steps * (-std::f64::consts::TAU / 128.0)) as f32
}

#[cfg(all(test, tests_rules_o_z))]
#[path = "phobos_jumpjet_tilt_tests.rs"]
mod tests;
