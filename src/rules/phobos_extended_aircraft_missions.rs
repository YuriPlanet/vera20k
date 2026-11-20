//! Phobos `ExtendedAircraftMissions.*`, the aircraft mission rework switch.
//!
//! `[General] ExtendedAircraftMissions=` (`Ext/Rules/Body.cpp:191`, a
//! `Valueable<bool>` default `no` at `Ext/Rules/Body.h:97`) is the master
//! switch for an aircraft type's reworked mission behaviour.
//! `[General] ExtendedAircraftMissions.UnlandDamage=`
//! (`Ext/Rules/Body.cpp:192`, `Valueable<int>` default `0` at
//! `Ext/Rules/Body.h:98`) is the damage an aircraft that cannot land takes,
//! or the kill/refusal threshold when negative.
//!
//! The type keys are read in `AircraftTypeExt::LoadFromINIFile`
//! (`Ext/AircraftType/Body.cpp:30-35`):
//! `ExtendedAircraftMissions=` (the type's own master override),
//! `.SmoothMoving`, `.EarlyDescend`, `.RearApproach`, `.FastScramble` (each
//! `Nullable<bool>`, `Ext/AircraftType/Body.h:21-25`) and `.UnlandDamage`
//! (`Nullable<int>`, `:26`).
//!
//! Each consumer resolves a type key with `key.Get(master)`, where `master`
//! is `type.Master.Get(RulesExt::Global()->ExtendedAircraftMissions)`:
//! `Ext/Aircraft/Hooks.cpp:513` (SmoothMoving), `:596`/`:701` (RearApproach),
//! `:671` (EarlyDescend), `:1090` (FastScramble) and `:772-777`/`:1049`
//! (UnlandDamage, which falls back to the **global** UnlandDamage directly,
//! not through the master switch).
//!
//! ## Dependency rules
//! - Part of rules/ — no dependencies on app/, render/, ui/, sim/ or audio/.

use crate::rules::ini_parser::IniSection;

/// The `[AircraftType]` `ExtendedAircraftMissions.*` block. A `None` keeps the
/// value the consumer's master/global fallback supplies.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct PhobosExtendedAircraftMissions {
    /// `ExtendedAircraftMissions=` (`Ext/AircraftType/Body.cpp:30`): the
    /// type's own master switch, overriding the `[General]` one.
    pub master: Option<bool>,
    /// `ExtendedAircraftMissions.SmoothMoving=` (`:31`).
    pub smooth_moving: Option<bool>,
    /// `ExtendedAircraftMissions.EarlyDescend=` (`:32`).
    pub early_descend: Option<bool>,
    /// `ExtendedAircraftMissions.RearApproach=` (`:33`).
    pub rear_approach: Option<bool>,
    /// `ExtendedAircraftMissions.FastScramble=` (`:34`).
    pub fast_scramble: Option<bool>,
    /// `ExtendedAircraftMissions.UnlandDamage=` (`:35`).
    pub unland_damage: Option<i32>,
}

impl PhobosExtendedAircraftMissions {
    /// Reads the six keys the native `AircraftTypeExt` reader takes
    /// (`Ext/AircraftType/Body.cpp:30-35`). `Nullable` semantics come from
    /// the section readers, so a blank/`none`/`<default>` value clears the
    /// entry back to unset.
    pub(crate) fn from_section(section: &IniSection) -> Self {
        Self {
            master: section.read_phobos_nullable_bool("ExtendedAircraftMissions", None),
            smooth_moving: section
                .read_phobos_nullable_bool("ExtendedAircraftMissions.SmoothMoving", None),
            early_descend: section
                .read_phobos_nullable_bool("ExtendedAircraftMissions.EarlyDescend", None),
            rear_approach: section
                .read_phobos_nullable_bool("ExtendedAircraftMissions.RearApproach", None),
            fast_scramble: section
                .read_phobos_nullable_bool("ExtendedAircraftMissions.FastScramble", None),
            unland_damage: section
                .read_phobos_nullable_int("ExtendedAircraftMissions.UnlandDamage", None),
        }
    }

    /// Resolves the block the way every hook reads it: the type's master or
    /// the `[General]` switch, then each type key or that master, and the
    /// type's `UnlandDamage` or the `[General]` value.
    pub fn resolve(
        &self,
        global_master: bool,
        global_unland_damage: i32,
    ) -> ExtendedAircraftMissionFlags {
        let master = self.master.unwrap_or(global_master);
        ExtendedAircraftMissionFlags {
            master,
            smooth_moving: self.smooth_moving.unwrap_or(master),
            early_descend: self.early_descend.unwrap_or(master),
            rear_approach: self.rear_approach.unwrap_or(master),
            fast_scramble: self.fast_scramble.unwrap_or(master),
            unland_damage: self.unland_damage.unwrap_or(global_unland_damage),
        }
    }
}

/// The resolved values the hooks branch on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExtendedAircraftMissionFlags {
    pub master: bool,
    pub smooth_moving: bool,
    pub early_descend: bool,
    pub rear_approach: bool,
    pub fast_scramble: bool,
    pub unland_damage: i32,
}

#[cfg(all(test, tests_rules_o_z))]
mod tests {
    use crate::rules::ini_parser::IniFile;
    use crate::rules::ruleset::RuleSet;

    fn test_rules(text: &str) -> RuleSet {
        RuleSet::from_ini(&IniFile::from_str(text)).expect("rules read")
    }

    #[test]
    fn type_master_overrides_the_global_and_the_keys_inherit_it() {
        let rules = test_rules(
            "[General]\nExtendedAircraftMissions=no\n\
             [AircraftTypes]\n0=TEST\n[TEST]\n\
             ExtendedAircraftMissions=yes\n\
             ExtendedAircraftMissions.UnlandDamage=7\n",
        );
        let ty = rules.object("TEST").unwrap();
        let flags = rules.extended_aircraft_missions_for(ty);
        assert!(flags.master);
        // The three plain keys inherit the type's master.
        assert!(flags.smooth_moving && flags.early_descend && flags.fast_scramble);
        // UnlandDamage falls back to the [General] value, which is 0 here, but
        // the type sets it explicitly.
        assert_eq!(flags.unland_damage, 7);
    }

    #[test]
    fn unset_type_keys_take_the_global_switch_and_value() {
        let rules = test_rules(
            "[General]\nExtendedAircraftMissions=yes\n\
             ExtendedAircraftMissions.UnlandDamage=-3\n\
             [AircraftTypes]\n0=TEST\n[TEST]\n",
        );
        let flags = rules.extended_aircraft_missions_for(rules.object("TEST").unwrap());
        assert!(flags.master && flags.rear_approach && flags.fast_scramble);
        assert_eq!(flags.unland_damage, -3);
    }

    #[test]
    fn an_explicit_key_does_not_follow_the_master() {
        let rules = test_rules(
            "[AircraftTypes]\n0=TEST\n[TEST]\n\
             ExtendedAircraftMissions=yes\n\
             ExtendedAircraftMissions.SmoothMoving=no\n\
             ExtendedAircraftMissions.RearApproach=no\n",
        );
        let flags = rules.extended_aircraft_missions_for(rules.object("TEST").unwrap());
        assert!(flags.master);
        assert!(!flags.smooth_moving && !flags.rear_approach);
        assert!(flags.early_descend);
    }

    /// Phobos keeps these in `AircraftTypeExt`, so a non-aircraft section that
    /// repeats the keys carries no value.
    #[test]
    fn only_aircraft_sections_carry_the_keys() {
        let rules = test_rules(
            "[VehicleTypes]\n0=TANK\n[TANK]\n\
             ExtendedAircraftMissions=yes\nExtendedAircraftMissions.UnlandDamage=5\n",
        );
        let flags = rules.extended_aircraft_missions_for(rules.object("TANK").unwrap());
        assert!(!flags.master && !flags.smooth_moving);
        assert_eq!(flags.unland_damage, 0);
    }
}
