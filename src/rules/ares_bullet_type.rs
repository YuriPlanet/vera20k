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
//! - `Splits`, `RetargetSelf`, `RetargetAccuracy`, `AirburstSpread` and
//!   `AroundTarget` (`+0x1C`..`+0x30`): the airburst and split children of
//!   BulletClass_DetonateAt_Splits (`1001A090`, `sim::combat::airburst`).
//!
//! ## Dependency rules
//! - Part of rules/ — no dependencies on sim/, render/, ui/, audio/, etc.

use crate::rules::ini_parser::IniSection;
use crate::util::native_x87::NativeF64Bits;

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
    /// `AnimLength` (`+0x44`, Valueable<int>, 0): above one, each of a
    /// rotating projectile's 32 facings owns this many frames, which play at
    /// `AnimRate` (BulletClass_GetAnimFrame, Ares.dll 1001A840, hook
    /// 0x468000).
    pub(crate) anim_length: i32,
    /// `Splits` (`+0x1C`, Valueable<bool>, false): split children target
    /// nearby Technos instead of each cell around the burst.
    pub(crate) splits: bool,
    /// `RetargetSelf` (`+0x1D`, Valueable<bool>, true): the firer may be a
    /// split target.
    pub(crate) retarget_self: bool,
    /// `RetargetAccuracy` (`+0x20`, Valueable<double>, 0.0).
    pub(crate) retarget_accuracy: NativeF64Bits,
    /// `AirburstSpread` (`+0x28`, Valueable<double>, 1.5): the radius in
    /// cells of a non-split airburst.
    pub(crate) airburst_spread: NativeF64Bits,
    /// `AroundTarget` (`+0x30`, Nullable<bool>; unset takes `Splits`).
    pub(crate) around_target: Option<bool>,
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
            anim_length: 0,
            splits: false,
            retarget_self: true,
            retarget_accuracy: NativeF64Bits::from_bits(0.0_f64.to_bits()),
            airburst_spread: NativeF64Bits::from_bits(1.5_f64.to_bits()),
            around_target: None,
        }
    }
}

impl BulletTypeExtRules {
    /// `100193C0`'s reads of these keys, in its order; absent, empty or
    /// invalid values keep the constructor's.
    pub(crate) fn from_section(section: &IniSection) -> Self {
        let _ares = crate::rules::ares_switch::ares_reads();
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
        ext.anim_length = section.read_ares_int("AnimLength", ext.anim_length);
        ext.airburst_spread = section.read_ares_double_bits("AirburstSpread", ext.airburst_spread);
        ext.retarget_accuracy =
            section.read_ares_double_bits("RetargetAccuracy", ext.retarget_accuracy);
        ext.splits = section.read_ares_bool("Splits", ext.splits);
        ext.retarget_self = section.read_ares_bool("RetargetSelf", ext.retarget_self);
        ext.around_target = section.read_ares_nullable_bool("AroundTarget", ext.around_target);
        // Retail projectiles already set the TS key.
        ext.ignores_firestorm = crate::rules::ares_switch::not_ares_read(|| {
            section.read_bool("IgnoresFirestorm", ext.ignores_firestorm)
        });
        ext
    }

    /// A rotating projectile's frame for the 32-step table frame `facing`
    /// (BulletClass_GetAnimFrame, Ares.dll 1001A840, hook 0x468000): with
    /// `AnimLength` above one, `facing * AnimLength + (frame / AnimRate) %
    /// AnimLength`, else `facing`. `AnimRate=0` divides by zero in the DLL
    /// (an integer fault); VERA keeps each facing's first frame instead.
    pub(crate) fn rotating_frame(&self, facing: i32, frame: i32, anim_rate: i32) -> i32 {
        let length = self.anim_length;
        if length < 2 {
            return facing;
        }
        let step = frame
            .checked_div(anim_rate)
            .map_or(0, |ticks| ticks % length);
        step.wrapping_add(length.wrapping_mul(facing))
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

    /// 100193C0's split keys over the constructor's Splits=false,
    /// RetargetSelf=true, RetargetAccuracy=0.0, AirburstSpread=1.5 and an
    /// unset AroundTarget; an invalid value keeps the current one.
    #[test]
    fn split_keys_read_over_the_constructor_defaults() {
        let ini = IniFile::from_str(
            "[SET]\nSplits=yes\nRetargetSelf=no\nRetargetAccuracy=0.25\n\
             AirburstSpread=2.5\nAroundTarget=no\n[BAD]\nSplits=maybe\nAroundTarget=x\n",
        );
        let set = BulletTypeExtRules::from_section(ini.section("SET").unwrap());
        assert!(set.splits && !set.retarget_self);
        assert_eq!(f64::from_bits(set.retarget_accuracy.bits()), 0.25);
        assert_eq!(f64::from_bits(set.airburst_spread.bits()), 2.5);
        assert_eq!(set.around_target, Some(false));
        let bad = BulletTypeExtRules::from_section(ini.section("BAD").unwrap());
        assert_eq!(bad, BulletTypeExtRules::default());
    }
}
