//! FLH (Forward/Lateral/Height) firing offset triplet.
//!
//! FLH defines where a projectile visually originates on a unit, relative to
//! the unit's turret pivot (or body center for turretless units).
//!
//! - **Forward**: along the unit's facing direction (positive = forward)
//! - **Lateral**: perpendicular to facing (positive = right of facing direction)
//! - **Height**: vertical offset (positive = up)
//!
//! Values are in leptons (256 leptons = 1 cell). Read from art.ini keys
//! (`PrimaryFireFLH=`, `SecondaryFireFLH=`, `ElitePrimaryFireFLH=`,
//! `EliteSecondaryFireFLH=`, `Weapon%dFLH=`, `AlternateFLH0..4=`) by the native coordinate
//! read, `IniSection::read_coord3`.
//!
//! ## Dependency rules
//! - Part of rules/ — no dependencies on sim/, render/, ui/, etc.

/// FLH (Forward, Lateral, Height) lepton offset triplet.
///
/// Defines where a projectile visually originates relative to the unit's
/// turret pivot (or body center for turretless units).
///
/// Values are in leptons (256 leptons = 1 cell).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Flh {
    /// Distance along the unit's facing direction. Positive = forward.
    pub forward: i32,
    /// Distance perpendicular to facing. Positive = right of facing direction.
    pub lateral: i32,
    /// Vertical offset. Positive = up.
    pub height: i32,
}

/// The art read's `[forward, lateral, height]` fields
/// ([`IniSection::read_coord3`](crate::rules::ini_parser::IniSection::read_coord3),
/// `0x00529CA0`).
impl From<[i32; 3]> for Flh {
    fn from([forward, lateral, height]: [i32; 3]) -> Self {
        Self {
            forward,
            lateral,
            height,
        }
    }
}

impl From<Flh> for [i32; 3] {
    fn from(flh: Flh) -> Self {
        [flh.forward, flh.lateral, flh.height]
    }
}
