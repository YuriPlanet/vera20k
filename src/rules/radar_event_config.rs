//! Radar event configuration parsed from rules.ini `[General]`.
//!
//! Controls the visual behavior of radar ping rectangles on the minimap.
//! Values from ModEnc: RadarEventMinRadius, RadarEventSpeed,
//! RadarEventRotationSpeed, RadarEventColorSpeed.
//!
//! ## Dependency rules
//! - Part of rules/ — depends only on rules/ini_parser.
//! - No dependencies on sim/, render/, ui/, etc.

use crate::rules::ini_parser::IniFile;
use crate::util::native_x87::NativeF32Bits;

/// `RulesClass::Constructor @ 0x00665650` seeds: `+0x7C` (`0x006656FA`),
/// `+0x80` (`0x006656FD`), `+0x84` (`0x00665707`), `+0x78` (`0x006656F3`).
const CTOR_MIN_RADIUS: i32 = 5;
const CTOR_SPEED: f32 = 1.0;
const CTOR_ROTATION_SPEED: f32 = 0.1;
const CTOR_COLOR_SPEED: f32 = 0.05;

/// The four radar-event scalar keys that the native runtime actually reads.
///
/// The three six-value duration/suppression arrays are parsed by gamemd but
/// never copied into its live 17-row event table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeRadarEventScalars {
    /// Native `int` field `+0x7C`.
    pub min_radius: i32,
    pub speed: NativeF32Bits,
    pub rotation_speed: NativeF32Bits,
    pub color_speed: NativeF32Bits,
}

impl Default for NativeRadarEventScalars {
    fn default() -> Self {
        Self {
            min_radius: CTOR_MIN_RADIUS,
            speed: NativeF32Bits::from_bits(CTOR_SPEED.to_bits()),
            rotation_speed: NativeF32Bits::from_bits(CTOR_ROTATION_SPEED.to_bits()),
            color_speed: NativeF32Bits::from_bits(CTOR_COLOR_SPEED.to_bits()),
        }
    }
}

impl NativeRadarEventScalars {
    /// The `[General]` reads, each over the constructor seed.
    pub fn from_ini(ini: &IniFile) -> Self {
        let ctor = Self::default();
        let general = ini.section_or_empty("General");
        Self {
            // `0x00671AB3` ReadInt -> `0x00671AB8 MOV [ESI+0x7C],EAX`.
            min_radius: general.read_int("RadarEventMinRadius", ctor.min_radius),
            // `0x006719C5` ReadDouble -> `0x006719CA FSTP dword [ESI+0x80]`.
            speed: general.read_double_to_float("RadarEventSpeed", ctor.speed),
            // `0x006719EA` ReadDouble -> `0x006719EF FSTP dword [ESI+0x84]`.
            rotation_speed: general
                .read_double_to_float("RadarEventRotationSpeed", ctor.rotation_speed),
            // `0x00671AD2` ReadDouble -> `0x00671AD7 FSTP dword [ESI+0x78]`.
            color_speed: general.read_double_to_float("RadarEventColorSpeed", ctor.color_speed),
        }
    }
}

/// Radar event visual parameters from `[General]`.
///
/// These control the animated radar ping rectangles that appear on the
/// minimap when combat or other events occur.
#[derive(Debug, Clone, Copy)]
pub struct RadarEventConfig {
    /// Final rectangle size (in minimap pixels) after the zoom-in animation.
    /// Larger = bigger final ping rectangle.
    pub min_radius: i32,
    /// Speed at which the ping rectangle shrinks from large to min_radius.
    /// Higher = faster zoom-in.
    #[cfg(any(test, feature = "test-support"))]
    pub speed: f32,
    /// Rotation speed of the ping rectangle (radians per native frame).
    pub rotation_speed: f32,
    /// Per-frame color-fade delta.
    pub color_speed: f32,
    /// Bit-exact representation consumed by the client radar-event tick.
    pub native_scalars: NativeRadarEventScalars,
}

impl Default for RadarEventConfig {
    /// The constructor seeds.
    fn default() -> Self {
        Self::from_scalars(NativeRadarEventScalars::default())
    }
}

impl RadarEventConfig {
    /// Parse radar event config from `[General]` section of rules.ini.
    pub fn from_ini(ini: &IniFile) -> Self {
        Self::from_scalars(NativeRadarEventScalars::from_ini(ini))
    }

    fn from_scalars(native_scalars: NativeRadarEventScalars) -> Self {
        Self {
            min_radius: native_scalars.min_radius,
            #[cfg(any(test, feature = "test-support"))]
            speed: f32::from_bits(native_scalars.speed.bits()),
            rotation_speed: f32::from_bits(native_scalars.rotation_speed.bits()),
            color_speed: f32::from_bits(native_scalars.color_speed.bits()),
            native_scalars,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_from_ini_overrides_defaults() {
        let ini = IniFile::from_str(
            "[General]\nRadarEventMinRadius=6.9\nRadarEventSpeed=0.15\n\
             RadarEventRotationSpeed=0.2\nRadarEventColorSpeed=0.1\n",
        );
        let config = RadarEventConfig::from_ini(&ini);
        // ReadInt: atoi stops at the '.'.
        assert_eq!(config.min_radius, 6);
        assert!((config.speed - 0.15).abs() < 0.01);
        assert!((config.rotation_speed - 0.2).abs() < 0.01);
        assert!((config.color_speed - 0.1).abs() < 0.01);
    }

    #[test]
    fn native_scalars_preserve_exact_stock_f32_bits() {
        let ini = IniFile::from_str(
            "[General]\n\
             RadarEventMinRadius=8\n\
             RadarEventSpeed=1.2\n\
             RadarEventRotationSpeed=.05\n\
             RadarEventColorSpeed=.1\n",
        );
        let scalars = NativeRadarEventScalars::from_ini(&ini);
        assert_eq!(scalars.min_radius, 8);
        assert_eq!(scalars.speed.bits(), 1.2_f32.to_bits());
        assert_eq!(scalars.rotation_speed.bits(), 0.05_f32.to_bits());
        assert_eq!(scalars.color_speed.bits(), 0.1_f32.to_bits());
    }

    #[test]
    fn dead_duration_arrays_do_not_change_live_scalars() {
        let baseline = NativeRadarEventScalars::from_ini(&IniFile::from_str(
            "[General]\nRadarEventSpeed=1.2\n",
        ));
        let modified = NativeRadarEventScalars::from_ini(&IniFile::from_str(
            "[General]\n\
             RadarEventSpeed=1.2\n\
             RadarEventSuppressionDistances=1,2,3,4,5,6\n\
             RadarEventVisibilityDurations=1,2,3,4,5,6\n\
             RadarEventDurations=6,5,4,3,2,1\n\
             RadarEventDuration=1\n",
        ));
        assert_eq!(modified, baseline);
    }
}
