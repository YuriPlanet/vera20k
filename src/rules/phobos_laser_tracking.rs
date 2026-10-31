//! Phobos `WeaponTypeExt::LaserPositionUpdate` (`src/Ext/WeaponType/Body.cpp`)
//! and the `RulesExt` default it inherits (`src/Ext/Rules/Body.cpp`).
//!
//! The key decides which ends of a weapon's LaserDraw follow their live
//! objects; the per-frame refresh itself is the simulation laser's retained
//! tracking data (`sim::combat::laser`).

use crate::rules::ini_parser::IniFile;

/// `PositionFollow` (`Enum.h`): the laser ends that follow an object. The
/// reader accepts exactly these four names, so the pair is not a flag list.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum PositionFollow {
    #[default]
    None,
    Firer,
    Target,
    All,
}

impl PositionFollow {
    /// `detail::read<PositionFollow>`: `_strcmpi` against the four names; an
    /// unknown name leaves the field unchanged.
    pub fn parse(raw: &str) -> Option<Self> {
        if raw.eq_ignore_ascii_case("none") {
            Some(Self::None)
        } else if raw.eq_ignore_ascii_case("firer") {
            Some(Self::Firer)
        } else if raw.eq_ignore_ascii_case("target") {
            Some(Self::Target)
        } else if raw.eq_ignore_ascii_case("all") {
            Some(Self::All)
        } else {
            None
        }
    }

    pub(crate) fn follows_firer(self) -> bool {
        matches!(self, Self::Firer | Self::All)
    }

    pub(crate) fn follows_target(self) -> bool {
        matches!(self, Self::Target | Self::All)
    }
}

/// `RulesExt`'s `LaserPositionUpdate.StopOnFirerConvert` (`Valueable<bool>`,
/// constructor default false, `[AudioVisual]`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct LaserTrackingGlobals {
    pub stop_on_firer_convert: bool,
}

impl LaserTrackingGlobals {
    pub fn from_ini(ini: &IniFile) -> Self {
        let audio_visual = ini.section_or_empty("AudioVisual");
        Self {
            stop_on_firer_convert: audio_visual
                .read_bool("LaserPositionUpdate.StopOnFirerConvert", false),
        }
    }
}

#[cfg(all(test, feature = "phobos"))]
#[path = "phobos_laser_tracking_tests.rs"]
mod tests;
