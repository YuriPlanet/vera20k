//! Phobos `OnlyUseLandSequences` and `SecondaryFireSequenceLandOnly`: an
//! infantry type's water-sequence remap overrides.
//!
//! ```ini
//! ; art section of the InfantryType
//! OnlyUseLandSequences=            ; Nullable<bool>, unset   Ext/InfantryType/Body.cpp:32
//! SecondaryFireSequenceLandOnly=   ; Nullable<bool>, unset   Ext/InfantryType/Body.cpp:33
//!
//! ; [General]
//! OnlyUseLandSequences=            ; Valueable<bool>, no     Ext/Rules/Body.cpp:622
//! SecondaryFireSequenceLandOnly=   ; Valueable<bool>, yes    Ext/Rules/Body.cpp:623
//! ```
//!
//! Declarations `Ext/InfantryType/Body.h:19-20` and `Ext/Rules/Body.h:520-521`,
//! constructors `InfantryType/Body.h:44-45` and `Rules/Body.h:1017-1018`.
//!
//! Both type keys are read from the **art** INI (`exArtINI, pArtSection`), not
//! the rules INI. VERA reads them at the per-pass fixed-ART owner
//! (`InfantryArt::read_pass`, `src/rules/object_type.rs`, the
//! `0x005246A1..0x0052472D` reads followed by the `0x52473F` hook) and
//! projects them onto `ObjectType::phobos_land_sequences`; a rules section
//! never supplies them.
//!
//! `InfantryClass_DoAction_Water` (`0x51D7E0`,
//! `Ext/Techno/Hooks.Misc.cpp:883-901`):
//!
//! ```cpp
//! if (pTypeExt->OnlyUseLandSequences.Get(RulesExt::Global()->OnlyUseLandSequences))
//!     return SkipWaterSequences;      // 0x51D842
//! if (sequence == Sequence::Walk || sequence == Sequence::Crawl)
//!     return UseSwim;                 // 0x51D83D
//! else if ((sequence == SecondaryFire || sequence == SecondaryProne)
//!          && pTypeExt->SecondaryFireSequenceLandOnly.Get(Global))
//!     return UseWetAttack;            // 0x51D82F (stock already maps 4/8 -> 0x16)
//! return Continue;                    // 0x51D7EC
//! ```
//!
//! The SecondaryFire/SecondaryProne remap is stock; the key (default yes)
//! only lets a mod turn it off. The consumer is
//! `sim::movement::infantry_action`'s water remap.
//!
//! ## Dependency rules
//! - Part of rules/ — no dependency on sim/, render/, ui/, etc.

use crate::rules::ini_parser::IniSection;

/// `OnlyUseLandSequences` on an `InfantryType`'s art section, plus the
/// `[General]` global it falls back to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct PhobosLandSequences {
    /// `OnlyUseLandSequences=`, `Nullable`: `None` keeps `[General]`.
    pub only_use_land_sequences: Option<bool>,
    /// `SecondaryFireSequenceLandOnly=`, `Nullable`: `None` keeps
    /// `[General] SecondaryFireSequenceLandOnly=` (default yes). No keeps
    /// the land SecondaryFire/SecondaryProne record in water.
    pub secondary_fire_sequence_land_only: Option<bool>,
}

impl PhobosLandSequences {
    /// One fresh read from an ART Image section
    /// (`Ext/InfantryType/Body.cpp:32`).
    pub fn from_section(section: &IniSection) -> Self {
        let mut this = Self::default();
        this.read_pass(section);
        this
    }

    /// One reached pass's `Nullable` read with the current value as the
    /// default, from the type Image's ART section. The per-pass owner calls
    /// this from `InfantryArt::read_pass`.
    pub fn read_pass(&mut self, section: &IniSection) {
        self.only_use_land_sequences =
            section.read_phobos_nullable_bool("OnlyUseLandSequences", self.only_use_land_sequences);
        self.secondary_fire_sequence_land_only = section.read_phobos_nullable_bool(
            "SecondaryFireSequenceLandOnly",
            self.secondary_fire_sequence_land_only,
        );
    }

    /// `OnlyUseLandSequences.Get(RulesExt::Global()->OnlyUseLandSequences)`
    /// (`Ext/Techno/Hooks.Misc.cpp:890`).
    pub fn only_land(self, global: bool) -> bool {
        self.only_use_land_sequences.unwrap_or(global)
    }

    /// `SecondaryFireSequenceLandOnly.Get(Global)` (`:895`): the resolved
    /// yes remaps an in-water SecondaryFire/SecondaryProne request to
    /// WetAttack (`0x51D82F`), which is stock; no keeps the land record.
    pub fn secondary_fire_land_only(self, global: bool) -> bool {
        self.secondary_fire_sequence_land_only.unwrap_or(global)
    }
}

#[cfg(all(test, tests_rules_o_z))]
#[path = "phobos_land_sequences_tests.rs"]
mod tests;
