//! Retained threat weights from original gamemd RulesClass::ReadGeneral66D530
//! and TechnoTypeClass::ReadINI712170.

use super::ini_parser::IniSection;
use crate::util::native_x87::NativeF64Bits;

/// Type+2C8..2E8, or Rules+1040..1060 for the corresponding defaults.
/// Bits preserve exceptional values and signed zero through saves and hashes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct ThreatCoefficients([NativeF64Bits; 5]);

impl Default for ThreatCoefficients {
    fn default() -> Self {
        // Original TechnoType ctor710AF0 (710B6A..710BA0) and
        // Rules ctor665650 (666C3F..666C75).
        Self([NativeF64Bits::POSITIVE_ZERO; 5])
    }
}

impl ThreatCoefficients {
    const KEYS: [&str; 5] = [
        "MyEffectivenessCoefficient",
        "TargetEffectivenessCoefficient",
        "TargetSpecialThreatCoefficient",
        "TargetStrengthCoefficient",
        "TargetDistanceCoefficient",
    ];

    /// Original Rules::ReadGeneral66D530 (671AF6..671B97): current defaults.
    pub(crate) fn read_defaults(&mut self, section: &IniSection) {
        for (key, value) in Self::KEYS.into_iter().zip(&mut self.0) {
            *value = section.read_double_bits(&format!("{key}Default"), *value);
        }
    }

    /// Original TechnoType::ReadINI712170 (71551E..715720), after admission.
    /// FCOMP0/FNSTSW/TEST AH,40h selects General when C3 is set: equal
    /// (including -0) or unordered under the game's masked x87 control word.
    pub(crate) fn read_type(&mut self, section: &IniSection, defaults: Self) {
        for ((key, value), fallback) in Self::KEYS.into_iter().zip(&mut self.0).zip(defaults.0) {
            // Classify stored bits independently of host denormal modes.
            let magnitude = value.bits() & 0x7fff_ffff_ffff_ffff;
            let default = if magnitude == 0 || magnitude > 0x7ff0_0000_0000_0000 {
                fallback
            } else {
                *value
            };
            *value = section.read_double_bits(key, default);
        }
    }

    pub(crate) fn my_effectiveness(self) -> f64 {
        f64::from_bits(self.0[0].bits())
    }

    pub(crate) fn target_effectiveness(self) -> f64 {
        f64::from_bits(self.0[1].bits())
    }

    pub(crate) fn target_special_threat(self) -> f64 {
        f64::from_bits(self.0[2].bits())
    }

    pub(crate) fn target_strength(self) -> f64 {
        f64::from_bits(self.0[3].bits())
    }

    pub(crate) fn target_distance(self) -> f64 {
        f64::from_bits(self.0[4].bits())
    }
}

#[cfg(test)]
#[path = "threat_coefficients_tests.rs"]
mod tests;
