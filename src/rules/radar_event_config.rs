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

/// The four radar-event scalar keys that the native runtime actually reads.
///
/// The three six-value duration/suppression arrays are parsed by gamemd but
/// never copied into its live 17-row event table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeRadarEventScalars {
    pub min_radius: NativeF32Bits,
    pub speed: NativeF32Bits,
    pub rotation_speed: NativeF32Bits,
    pub color_speed: NativeF32Bits,
}

impl Default for NativeRadarEventScalars {
    fn default() -> Self {
        Self {
            min_radius: NativeF32Bits::from_bits(8.0_f32.to_bits()),
            speed: NativeF32Bits::from_bits(1.2_f32.to_bits()),
            rotation_speed: NativeF32Bits::from_bits(0.05_f32.to_bits()),
            color_speed: NativeF32Bits::from_bits(0.1_f32.to_bits()),
        }
    }
}

impl NativeRadarEventScalars {
    pub fn from_ini(ini: &IniFile) -> Self {
        let defaults = Self::default();
        let Some(general) = ini.section("General") else {
            return defaults;
        };
        Self {
            min_radius: general
                .get_f32("RadarEventMinRadius")
                .map_or(defaults.min_radius, |value| {
                    NativeF32Bits::from_bits(value.to_bits())
                }),
            speed: general
                .get_f32("RadarEventSpeed")
                .map_or(defaults.speed, |value| {
                    NativeF32Bits::from_bits(value.to_bits())
                }),
            rotation_speed: general
                .get_f32("RadarEventRotationSpeed")
                .map_or(defaults.rotation_speed, |value| {
                    NativeF32Bits::from_bits(value.to_bits())
                }),
            color_speed: general
                .get_f32("RadarEventColorSpeed")
                .map_or(defaults.color_speed, |value| {
                    NativeF32Bits::from_bits(value.to_bits())
                }),
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
    /// Stock default 8. Larger = bigger final ping rectangle.
    pub min_radius: f32,
    /// Speed at which the ping rectangle shrinks from large to min_radius.
    /// Stock default 1.2. Higher = faster zoom-in.
    pub speed: f32,
    /// Rotation speed of the ping rectangle (radians per native frame).
    /// Stock default 0.05.
    pub rotation_speed: f32,
    /// Per-frame color-fade delta.
    /// Stock default 0.1.
    pub color_speed: f32,
    /// Bit-exact representation consumed by the client radar-event tick.
    pub native_scalars: NativeRadarEventScalars,
}

impl Default for RadarEventConfig {
    fn default() -> Self {
        Self {
            min_radius: 8.0,
            speed: 1.2,
            rotation_speed: 0.05,
            color_speed: 0.1,
            native_scalars: NativeRadarEventScalars::default(),
        }
    }
}

impl RadarEventConfig {
    /// Parse radar event config from `[General]` section of rules.ini.
    pub fn from_ini(ini: &IniFile) -> Self {
        if ini.section("General").is_none() {
            return Self::default();
        }
        let native_scalars = NativeRadarEventScalars::from_ini(ini);
        Self {
            min_radius: f32::from_bits(native_scalars.min_radius.bits()),
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
    fn default_config_has_reasonable_values() {
        let config = RadarEventConfig::default();
        assert!(config.min_radius > 0.0);
        assert!(config.speed > 0.0);
    }

    #[test]
    fn parse_from_ini_overrides_defaults() {
        let ini = IniFile::from_str(
            "[General]\nRadarEventMinRadius=6.0\nRadarEventSpeed=0.15\n\
             RadarEventRotationSpeed=0.2\nRadarEventColorSpeed=0.1\n",
        );
        let config = RadarEventConfig::from_ini(&ini);
        assert!((config.min_radius - 6.0).abs() < 0.01);
        assert!((config.speed - 0.15).abs() < 0.01);
        assert!((config.rotation_speed - 0.2).abs() < 0.01);
        assert!((config.color_speed - 0.1).abs() < 0.01);
    }

    #[test]
    fn missing_general_section_uses_defaults() {
        let ini = IniFile::from_str("[Map]\nTheater=TEMPERATE\n");
        let config = RadarEventConfig::from_ini(&ini);
        assert!((config.min_radius - 8.0).abs() < 0.01);
        assert!((config.speed - 1.2).abs() < 0.01);
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
        assert_eq!(scalars.min_radius.bits(), 8.0_f32.to_bits());
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
