//! Optional Ares BulletTypeExt keys, read by
//! `BulletTypeExt::ExtData::LoadFromINIFile` (installed Ares `100193C0`)
//! from the projectile's rules section with the projectile
//! (`rules::projectile_type`).
//!
//! - `SubjectToBuildings` and `SolidLevel` (`+0x8`, `+0xC`): buildings on
//!   the line of fire and which of them a shot may pass
//!   (`sim::combat::line_of_fire`, `AresTrajectoryHelper` `1006E1F0`,
//!   `1006E2B0`, `1006E540`).
//! - `SubjectToTrenches` (`+0x11`): a false one always passes a shot into a
//!   garrison's occupants (`sim::ares_garrison`).
//! - `BallisticScatter.Min` and `.Max` (`+0x34`, `+0x3C`): FireAt's launch
//!   scatter magnitude (`10019930`, `10019980`).
//! - The stock ObjectType `IgnoresFirestorm=`, which only the Ares firestorm
//!   walls read of a projectile (`sim::ares_firestorm`).
//!
//! ## Dependency rules
//! - Part of rules/ — no dependencies on sim/, render/, ui/, audio/, etc.

use crate::rules::ini_parser::IniSection;

/// The BulletTypeExt fields VERA consumes. The constructor
/// (`BulletTypeExt::ExtMap.FindOrAllocate @ 100192C0`) clears
/// `SubjectToBuildings` and `SolidLevel`, sets `SubjectToTrenches`, and
/// leaves both scatter bounds unset.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct BulletTypeExtRules {
    /// `SubjectToBuildings` (`+0x8`, Valueable<bool>): buildings on the line
    /// block the shot (Ares `SubjectToSolid`).
    pub(crate) subject_to_solid: bool,
    /// `SolidLevel` (`+0xC`, Valueable<int>): an enemy building whose own
    /// `SolidLevel=` is at most this does not stop the shot.
    pub(crate) solid_level: i32,
    /// `SubjectToTrenches` (`+0x11`, Valueable<bool>).
    pub(crate) subject_to_trenches: bool,
    /// `BallisticScatter.Min` (`+0x34`, Nullable<Leptons>, read in cells).
    pub(crate) ballistic_scatter_min: Option<i32>,
    /// `BallisticScatter.Max` (`+0x3C`, Nullable<Leptons>).
    pub(crate) ballistic_scatter_max: Option<i32>,
    /// ObjectType `IgnoresFirestorm=` (`+0x239`, constructor false), which
    /// `ObjectTypeClass::ReadINI @ 0x005F92D0` reads with ReadBool; the Ares
    /// firestorm checks of a shot read it (`10024C00`, `1001A940`,
    /// `1001AC90`).
    pub(crate) ignores_firestorm: bool,
}

impl Default for BulletTypeExtRules {
    fn default() -> Self {
        Self {
            subject_to_solid: false,
            solid_level: 0,
            subject_to_trenches: true,
            ballistic_scatter_min: None,
            ballistic_scatter_max: None,
            ignores_firestorm: false,
        }
    }
}

impl BulletTypeExtRules {
    /// `100193C0`'s reads of these keys, in its order; absent, empty or
    /// invalid values keep the constructor's.
    pub(crate) fn from_section(section: &IniSection) -> Self {
        let mut ext = Self::default();
        ext.subject_to_solid = section.read_ares_bool("SubjectToBuildings", ext.subject_to_solid);
        ext.solid_level = section.read_ares_int("SolidLevel", ext.solid_level);
        ext.subject_to_trenches =
            section.read_ares_bool("SubjectToTrenches", ext.subject_to_trenches);
        // Nullable<Leptons>::Read 10019660.
        ext.ballistic_scatter_min =
            section.read_ares_nullable_leptons("BallisticScatter.Min", ext.ballistic_scatter_min);
        ext.ballistic_scatter_max =
            section.read_ares_nullable_leptons("BallisticScatter.Max", ext.ballistic_scatter_max);
        ext.ignores_firestorm = section.read_bool("IgnoresFirestorm", ext.ignores_firestorm);
        ext
    }

    /// FireAt's launch-scatter magnitude bounds for `RandomRanged`
    /// (`TechnoClass_Fire_BallisticScatter1 @ 10019930`, hook `0x6FE709`,
    /// for the flak arm; `...Scatter2 @ 10019980`, hook `0x6FE7FE`, for the
    /// plain arm): `Min.Get(0)` or `Min.Get(BallisticScatter / 2)`, then
    /// `Max.Get(BallisticScatter)`.
    pub(crate) fn launch_scatter_bounds(&self, flak: bool, ballistic_scatter: i32) -> (i32, i32) {
        let low = if flak { 0 } else { ballistic_scatter / 2 };
        (
            self.ballistic_scatter_min.unwrap_or(low),
            self.ballistic_scatter_max.unwrap_or(ballistic_scatter),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;

    /// `BallisticScatter.Min`/`.Max` are read in cells (`Nullable<Leptons>`
    /// 10019660) and override only their own bound of each arm's draw
    /// (10019930 for flak, 10019980 for the plain arm); unset bounds keep
    /// the original `0`/`BallisticScatter / 2` and `BallisticScatter`.
    #[test]
    fn ballistic_scatter_bounds_override_each_arm_separately() {
        let ini = IniFile::from_str(
            "[MIN]\nBallisticScatter.Min=1\nBallisticScatter.Max=bad\n\
             [BOTH]\nBallisticScatter.Min=0.5\nBallisticScatter.Max=2.5\n[NONE]\nArm=1\n",
        );
        let ext = |name| BulletTypeExtRules::from_section(ini.section(name).unwrap());
        assert_eq!(ext("NONE").launch_scatter_bounds(true, 0x101), (0, 0x101));
        assert_eq!(
            ext("NONE").launch_scatter_bounds(false, 0x101),
            (0x80, 0x101)
        );
        assert_eq!(ext("MIN").launch_scatter_bounds(false, 0x101), (256, 0x101));
        assert_eq!(ext("BOTH").launch_scatter_bounds(true, 0x101), (128, 640));
    }
}
