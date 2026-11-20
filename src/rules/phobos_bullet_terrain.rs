//! Phobos `BulletTypeExt` land/water obstacle switches: `SubjectToLand`,
//! `SubjectToLand.Detonate` and their water twins
//! (`Ext/BulletType/Body.cpp:48-51`, declared `Ext/BulletType/Body.h:46-49`).
//!
//! `SubjectToLand`/`SubjectToWater` are `Nullable<bool>` (unset by default);
//! the two `.Detonate` companions are `Valueable<bool>` constructed **true**
//! (`Ext/BulletType/Body.h:115-118`). `Ext/Bullet/Hooks.Obstacles.cpp:74-87`
//! reads them per walked cell: with either nullable set the bullet takes the
//! obstacle walk even when its `Level` is clear, a non-water cell the land
//! nullable accepts blocks/detonates per `.Detonate`, and the water pair does
//! the same for `Water`/`Beach` cells.
//!
//! VERA's pre-shot obstacle walk (`sim::combat::line_of_fire`) feeds both
//! range checks and bullet AI from one gate, so this module stops at the read;
//! splitting that walk by caller is the separate landing chain.
//!
//! ## Dependency rules
//! - Part of rules/ — no dependency on sim/, render/, ui/, etc.

use crate::rules::ini_parser::IniSection;

/// The four land/water keys' constructor values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PhobosSubjectToTerrain {
    /// `SubjectToLand=` (`Nullable<bool>`, unset).
    pub subject_to_land: Option<bool>,
    /// `SubjectToLand.Detonate=` (`Valueable<bool>`, true).
    pub subject_to_land_detonate: bool,
    /// `SubjectToWater=` (`Nullable<bool>`, unset).
    pub subject_to_water: Option<bool>,
    /// `SubjectToWater.Detonate=` (`Valueable<bool>`, true).
    pub subject_to_water_detonate: bool,
}

impl Default for PhobosSubjectToTerrain {
    fn default() -> Self {
        Self {
            subject_to_land: None,
            subject_to_land_detonate: true,
            subject_to_water: None,
            subject_to_water_detonate: true,
        }
    }
}

impl PhobosSubjectToTerrain {
    pub fn from_section(section: &IniSection) -> Self {
        Self {
            subject_to_land: section.read_phobos_nullable_bool("SubjectToLand", None),
            subject_to_land_detonate: section.read_bool("SubjectToLand.Detonate", true),
            subject_to_water: section.read_phobos_nullable_bool("SubjectToWater", None),
            subject_to_water_detonate: section.read_bool("SubjectToWater.Detonate", true),
        }
    }

    /// `true` when either nullable makes the bullet take part in the
    /// land/water obstacle walk (`Hooks.Obstacles.cpp:69`).
    pub fn any_subject_to_terrain(self) -> bool {
        self.subject_to_land.is_some() || self.subject_to_water.is_some()
    }
}

#[cfg(all(test, tests_rules_o_z))]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;

    #[test]
    fn the_bullet_land_water_keys_default_to_unset_and_true_detonate() {
        let ini = IniFile::from_str("[BULLET]\nStart=0\n");
        let section = ini.section("BULLET").unwrap();
        let read = PhobosSubjectToTerrain::from_section(section);
        assert_eq!(read, PhobosSubjectToTerrain::default());
        assert_eq!(read.subject_to_land, None);
        assert!(read.subject_to_land_detonate);
        assert_eq!(read.subject_to_water, None);
        assert!(read.subject_to_water_detonate);
        assert!(!read.any_subject_to_terrain());
    }

    #[test]
    fn the_bullet_land_water_keys_read_their_authored_values() {
        let ini = IniFile::from_str(
            "[BULLET]\nSubjectToLand=no\nSubjectToLand.Detonate=no\n\
             SubjectToWater=yes\nSubjectToWater.Detonate=no\n",
        );
        let section = ini.section("BULLET").unwrap();
        let read = PhobosSubjectToTerrain::from_section(section);
        assert_eq!(read.subject_to_land, Some(false));
        assert!(!read.subject_to_land_detonate);
        assert_eq!(read.subject_to_water, Some(true));
        assert!(!read.subject_to_water_detonate);
        assert!(read.any_subject_to_terrain());
    }
}
